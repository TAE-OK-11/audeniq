//! Partner acknowledgement documents: DDEX choreography ACK files
//! (`FtpAcknowledgementMessage` and look-alikes), partner result files and
//! JSON webhook bodies, normalized to one shape.
//!
//! Status vocabularies differ per partner; the mapping below covers the
//! DDEX ERN choreography values and the common English words. Anything
//! unrecognized that is not clearly "in progress" counts as a failure with
//! the partner's own value as the code, so a new rejection reason is never
//! mistaken for success.
use crate::execution::AckEvent;
use crate::transport::xml_values;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckStatus {
    Success,
    /// Ingested and published (explicit live signal).
    Live,
    TakenDown,
    Pending,
    Failure {
        code: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckDoc {
    pub status: AckStatus,
    /// File names, paths, message/batch/release ids the document mentions.
    pub refs: Vec<String>,
    pub event_id: Option<String>,
    pub partner_release_id: Option<String>,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Map one partner status word.
pub fn classify_status(raw: &str, text: &str) -> AckStatus {
    match norm(raw).as_str() {
        "fileok"
        | "processedsuccessfully"
        | "success"
        | "successful"
        | "succeeded"
        | "ok"
        | "accepted"
        | "ingested"
        | "delivered"
        | "complete"
        | "completed"
        | "valid" => AckStatus::Success,
        "live" | "published" | "online" | "available" => AckStatus::Live,
        "takendown" | "removed" | "takedownconfirmed" | "withdrawn" => AckStatus::TakenDown,
        "" | "received" | "pending" | "processing" | "inprogress" | "queued" | "waiting" => {
            AckStatus::Pending
        }
        _ => AckStatus::Failure {
            code: raw.trim().chars().take(80).collect(),
            text: text.trim().chars().take(400).collect(),
        },
    }
}

/// Parse an ACK/result document (XML or JSON). None: not a recognizable
/// acknowledgement at all.
pub fn parse_document(bytes: &[u8]) -> Option<AckDoc> {
    let text = std::str::from_utf8(bytes)
        .ok()?
        .trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        let v: Value = serde_json::from_str(trimmed).ok()?;
        return Some(parse_json(&v));
    }
    if !trimmed.starts_with('<') {
        return None;
    }
    // Well-formedness first: a truncated file is not an answer yet.
    let mut r = quick_xml::Reader::from_str(trimmed);
    loop {
        match r.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    let first = |tags: &[&str]| {
        tags.iter()
            .find_map(|t| xml_values(trimmed, t).into_iter().find(|v| !v.is_empty()))
            .unwrap_or_default()
    };
    // A file-level error anywhere wins over an overall status.
    let file_statuses: Vec<String> = xml_values(trimmed, "FileStatus");
    let overall = first(&[
        "MessageStatus",
        "Status",
        "ReleaseStatus",
        "ProcessingStatus",
    ]);
    let text_msg = first(&["ErrorText", "ErrorMessage", "StatusMessage", "Description"]);
    let status = file_statuses
        .iter()
        .map(|s| classify_status(s, &text_msg))
        .find(|s| matches!(s, AckStatus::Failure { .. }))
        .unwrap_or_else(|| {
            if overall.is_empty() {
                file_statuses
                    .first()
                    .map(|s| classify_status(s, &text_msg))
                    .unwrap_or(AckStatus::Pending)
            } else {
                classify_status(&overall, &text_msg)
            }
        });
    let mut refs = Vec::new();
    for tag in [
        "FileName",
        "FilePath",
        "MessageId",
        "MessageThreadId",
        "BatchId",
        "ICPN",
        "UPC",
        "GRid",
        "ProprietaryId",
    ] {
        refs.extend(
            xml_values(trimmed, tag)
                .into_iter()
                .filter(|v| !v.is_empty()),
        );
    }
    Some(AckDoc {
        status,
        refs,
        event_id: None,
        partner_release_id: Some(first(&["PartnerReleaseId", "ReleaseReference"]))
            .filter(|s| !s.is_empty()),
    })
}

fn parse_json(v: &Value) -> AckDoc {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let raw = [s("type"), s("status"), s("result"), s("state")]
        .into_iter()
        .find(|x| !x.is_empty())
        .unwrap_or_default();
    let code = [s("code"), s("error_code"), s("reason")]
        .into_iter()
        .find(|x| !x.is_empty())
        .unwrap_or_default();
    let mut status = classify_status(&raw, &s("message"));
    if let AckStatus::Failure { code: c, text } = &mut status {
        if !code.is_empty() {
            *c = code;
        }
        if text.is_empty() {
            *text = s("message");
        }
    }
    let mut refs = Vec::new();
    for k in [
        "partner_message_id",
        "delivery_id",
        "batch_id",
        "message_id",
        "file",
        "upc",
    ] {
        let x = s(k);
        if !x.is_empty() {
            refs.push(x);
        }
    }
    AckDoc {
        status,
        refs,
        event_id: Some(s("event_id")).filter(|x| !x.is_empty()),
        partner_release_id: [s("partner_release_id"), s("release_id")]
            .into_iter()
            .find(|x| !x.is_empty()),
    }
}

/// Stable event id for a document without one (ACK files): its hash.
pub fn content_event_id(bytes: &[u8]) -> String {
    format!("doc:{}", &crate::domain::sha256_hex(bytes)[..32])
}

/// Turn a parsed document into an execution event for `partner_message_id`.
/// None for pending documents.
pub fn to_event(doc: &AckDoc, bytes: &[u8], partner_message_id: &str) -> Option<AckEvent> {
    let event_id = doc
        .event_id
        .clone()
        .unwrap_or_else(|| content_event_id(bytes));
    let pmid = partner_message_id.to_string();
    Some(match &doc.status {
        AckStatus::Success => AckEvent::Accepted {
            event_id,
            partner_message_id: pmid,
        },
        AckStatus::Live => AckEvent::Live {
            event_id,
            partner_release_id: doc
                .partner_release_id
                .clone()
                .unwrap_or_else(|| pmid.clone()),
            partner_message_id: Some(pmid),
        },
        AckStatus::TakenDown => AckEvent::TakedownConfirmed {
            event_id,
            partner_release_id: doc.partner_release_id.clone(),
            partner_message_id: Some(pmid),
        },
        AckStatus::Failure { code, .. } => AckEvent::Rejected {
            event_id,
            partner_message_id: pmid,
            code: code.clone(),
        },
        AckStatus::Pending => return None,
    })
}

/// DDEX batch-profile message id (`<17-digit batch>/<UPC>`) or
/// release-by-release folder (`<UPC>_<17-digit ts>`) mentioned in any ref.
pub fn find_ddex_message_id(refs: &[String]) -> Option<String> {
    let digits = |s: &str, lo: usize, hi: usize| {
        (lo..=hi).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    for r in refs {
        let segs: Vec<&str> = r.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        for w in segs.windows(2) {
            if digits(w[0], 17, 17) && digits(w[1], 8, 14) {
                return Some(format!("{}/{}", w[0], w[1]));
            }
        }
        for s in &segs {
            if let Some((a, b)) = s.split_once('_')
                && digits(a, 8, 14)
                && digits(b, 17, 17)
            {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FTP_ACK_OK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ernc:FtpAcknowledgementMessage xmlns:ernc="http://ddex.net/xml/ern-c/15">
  <MessageHeader><MessageId>ACK-1</MessageId></MessageHeader>
  <AcknowledgedFile><FileName>8800000000017.xml</FileName>
    <FilePath>/inbox/20260928120000123/8800000000017/</FilePath></AcknowledgedFile>
  <MessageStatus>FileOK</MessageStatus>
</ernc:FtpAcknowledgementMessage>"#;

    #[test]
    fn ddex_ftp_ack_success_and_message_id() {
        let d = parse_document(FTP_ACK_OK.as_bytes()).unwrap();
        assert_eq!(d.status, AckStatus::Success);
        assert_eq!(
            find_ddex_message_id(&d.refs).as_deref(),
            Some("20260928120000123/8800000000017")
        );
        let e = to_event(&d, FTP_ACK_OK.as_bytes(), "20260928120000123/8800000000017").unwrap();
        assert!(matches!(e, AckEvent::Accepted { .. }));
    }

    #[test]
    fn file_level_error_wins_and_keeps_partner_code() {
        let x = r#"<Ack><MessageStatus>FileOK</MessageStatus>
          <FileStatus>FileOK</FileStatus><FileStatus>ResourceCorrupt</FileStatus>
          <ErrorText>checksum mismatch on 8800000000017_01_001.flac</ErrorText></Ack>"#;
        match parse_document(x.as_bytes()).unwrap().status {
            AckStatus::Failure { code, text } => {
                assert_eq!(code, "ResourceCorrupt");
                assert!(text.contains("checksum"));
            }
            s => panic!("{s:?}"),
        }
    }

    #[test]
    fn truncated_or_foreign_documents_are_not_answers() {
        assert!(parse_document(b"<Ack><MessageStatus>FileOK</Mess").is_none());
        assert!(parse_document(b"plain text").is_none());
        let pending = parse_document(br#"{"status":"processing"}"#).unwrap();
        assert_eq!(pending.status, AckStatus::Pending);
        assert!(to_event(&pending, b"x", "m").is_none());
    }

    #[test]
    fn json_results_map_codes_and_release_ids() {
        let d = parse_document(
            r#"{"event_id":"e1","status":"rejected","code":"ART_TEXT","message":"cover has text"}"#
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            d.status,
            AckStatus::Failure {
                code: "ART_TEXT".into(),
                text: "cover has text".into()
            }
        );
        let live = parse_document(br#"{"status":"live","release_id":"KR-123"}"#).unwrap();
        match to_event(&live, b"x", "8800000000017_20260928120000123").unwrap() {
            AckEvent::Live {
                partner_release_id,
                partner_message_id,
                ..
            } => {
                assert_eq!(partner_release_id, "KR-123");
                assert_eq!(
                    partner_message_id.as_deref(),
                    Some("8800000000017_20260928120000123")
                );
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(
            find_ddex_message_id(&["x/8800000000017_20260928120000123/y".into()]).as_deref(),
            Some("8800000000017_20260928120000123")
        );
        assert!(find_ddex_message_id(&["x/y".into()]).is_none());
    }
}
