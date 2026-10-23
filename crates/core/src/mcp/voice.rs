//! AUDENIQ's own Korean wording for common tool outcomes (`message_ko`).
//!
//! Assistants answer in their own words, so the same result reads differently
//! in ChatGPT, Claude or Grok. Each result whose meaning fits one sentence
//! carries the sentence AUDENIQ would say, in its voice (해요체, active voice
//! for what was done, no hype), and the server instructions ask Korean
//! replies to start from it. Amounts, declarations and review results are
//! never paraphrased here; those stay in the structured fields.
use serde_json::Value;

/// `word` with the particle its last syllable takes: `josa("멤버 초대", "을",
/// "를")` is "멤버 초대를". Sentences never fall back to "을(를)". Only for
/// Korean words; names in other scripts go where no particle follows
/// ("ChatGPT에서").
pub(super) fn josa(word: &str, after_consonant: &str, after_vowel: &str) -> String {
    let consonant = word
        .chars()
        .rev()
        .find(|c| !c.is_whitespace())
        .filter(|c| ('가'..='힣').contains(c))
        .is_some_and(|c| !(c as u32 - '가' as u32).is_multiple_of(28));
    format!(
        "{word}{}",
        if consonant {
            after_consonant
        } else {
            after_vowel
        }
    )
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The sentence for a tool result, or `None` when the result needs the
/// assistant to explain details (lists, previews, reports).
pub fn message_ko(v: &Value) -> Option<String> {
    let text = match s(v, "view") {
        "permission_request" => match s(v, "status") {
            "PENDING" => {
                "AUDENIQ Studio에 권한 요청을 보냈어요. 아래 링크에서 승인해 주세요.".into()
            }
            "APPROVED" => "권한이 더해졌어요. 이어서 진행할게요.".into(),
            "ALREADY_GRANTED" => "이미 필요한 권한이 있어요.".into(),
            "REJECTED" => "권한 요청을 거절했어요. 지금 권한으로 할 수 있는 일만 할게요.".into(),
            "EXPIRED" => "권한 요청 시간이 지났어요. 필요하면 다시 요청할게요.".into(),
            "WITHDRAWN" => "권한 요청을 취소했어요. 지금 권한 그대로예요.".into(),
            _ => return None,
        },
        "action_request" => {
            let title = v["summary"]["title"].as_str().unwrap_or("요청한 작업");
            match s(v, "status") {
                "PENDING" => format!(
                    "AUDENIQ Studio에서 {} 승인해 주세요. 승인하기 전에는 아무것도 바뀌지 않아요.",
                    josa(title, "을", "를")
                ),
                "DONE" => format!("{} 마쳤어요.", josa(title, "을", "를")),
                "FAILED" => format!(
                    "승인했지만 {} 마치지 못했어요. 이유를 확인하고 다시 진행할게요.",
                    josa(title, "을", "를")
                ),
                "REJECTED" => format!(
                    "요청을 거절했어요. {} 진행되지 않아요.",
                    josa(title, "은", "는")
                ),
                "EXPIRED" => format!(
                    "승인 기한이 지나서 {} 진행되지 않았어요. 필요하면 다시 요청할게요.",
                    josa(title, "은", "는")
                ),
                "WITHDRAWN" => format!(
                    "요청을 취소했어요. {} 진행되지 않아요.",
                    josa(title, "은", "는")
                ),
                _ => return None,
            }
        }
        "upload_status" => match s(v, "state") {
            "READY" => "파일을 올렸어요. 발매에 붙일게요.".into(),
            "PROCESSING" => "파일을 확인하고 있어요. 조금만 기다려 주세요.".into(),
            "WAITING_FOR_FILE" => "아직 파일이 도착하지 않았어요.".into(),
            "EXPIRED" => "업로드 시간이 지났어요. 파일을 다시 올려 주세요.".into(),
            "FAILED" => "파일을 확인하지 못했어요. 다른 파일로 다시 올려 주세요.".into(),
            _ => return None,
        },
        "pending_approval" => match s(v, "approval_status") {
            "WITHDRAWN" => "승인을 기다리던 신청을 취소했어요. 접수되지 않았어요.".into(),
            "PENDING" | "" => {
                "신청서를 보냈어요. AUDENIQ Studio에서 한 번 더 승인하면 접수돼요.".into()
            }
            _ => return None,
        },
        "submitted" => "발매를 신청했어요. AUDENIQ이 검토를 시작해요.".into(),
        "release_deleted" => "초안을 삭제했어요.".into(),
        "notifications_read" => "알림을 읽음으로 표시했어요.".into(),
        "document_proof" => "증빙 파일을 붙였어요. AUDENIQ이 확인할게요.".into(),
        "special_options" => "특수 콘텐츠 정보를 저장했어요.".into(),
        "split_invite_resent" => "분배 수락 메일을 다시 보냈어요.".into(),
        "member_invite" if s(v, "status") == "CANCELLED" => "초대를 취소했어요.".into(),
        "counter_notice" if v["counter_withdrawn"] == true => {
            "이의 제기를 철회했어요. 발매는 계속 중단돼 있어요.".into()
        }
        "document_proof_withdrawn" => {
            "보낸 증빙 파일을 회수했어요. 맞는 파일을 다시 붙여 주세요.".into()
        }
        "takedown" if s(v, "status") == "CANCELLED" => "서비스 중단 요청을 취소했어요.".into(),
        "validation" => {
            if v["ready_to_submit"] == true {
                "신청 전에 확인할 항목을 모두 채웠어요.".into()
            } else {
                let n = v["blocking"].as_array().map_or(0, Vec::len);
                format!("신청 전에 채울 항목이 {n}개 남았어요.")
            }
        }
        _ => return None,
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The Studio's UX writing rules (web/studio/app/src/lib/writing.test.ts)
    /// for every Korean sentence the MCP sends: results, notices, approval
    /// pages, prompts and the widget. The server instructions quote the
    /// rules as examples, so they are left out.
    #[test]
    fn korean_text_follows_the_writing_rules() {
        let sources = [
            ("actions.rs", include_str!("actions.rs")),
            ("approvals.rs", include_str!("approvals.rs")),
            ("connect.rs", include_str!("connect.rs")),
            ("files.rs", include_str!("files.rs")),
            ("login.rs", include_str!("login.rs")),
            ("oauth.rs", include_str!("oauth.rs")),
            ("prompts.rs", include_str!("prompts.rs")),
            ("public.rs", include_str!("public.rs")),
            ("safety.rs", include_str!("safety.rs")),
            ("tools.rs", include_str!("tools.rs")),
            ("workspace.rs", include_str!("workspace.rs")),
            ("widget.html", include_str!("widget.html")),
        ];
        let rules: [(&[&str], &str); 7] = [
            (&["님께", "분께"], "'께' 대신 '에게'"),
            (&["되었", "되어요"], "'됐어요'·'돼요'"),
            (&["시겠어요", "하시겠", "하십시오"], "해요체"),
            (&["해주세요", "해주시"], "'해 주세요'"),
            (&["미입력", "미등록"], "'입력 전'·'등록 전'"),
            (&["해당 "], "'그 ~'"),
            (
                &["을(를)", "이(가)", "은(는)", "와(과)", "(으)로"],
                "조사는 voice::josa로",
            ),
        ];
        let hangul = |l: &str| l.chars().any(|c| ('가'..='힣').contains(&c));
        let mut hits = Vec::new();
        for (file, src) in sources {
            for (n, line) in src.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                // Tests that check for these words name them.
                if !hangul(code) || code.contains("assert") {
                    continue;
                }
                for (words, why) in &rules {
                    if words.iter().any(|w| code.contains(w)) {
                        hits.push(format!("{file}:{} {why}: {}", n + 1, code.trim()));
                    }
                }
            }
        }
        assert!(hits.is_empty(), "{hits:#?}");
    }

    #[test]
    fn outcomes_read_in_audeniq_voice() {
        assert_eq!(
            message_ko(&json!({"view":"action_request","status":"PENDING","summary":{"title":"서비스 중단 요청"}})).unwrap(),
            "AUDENIQ Studio에서 서비스 중단 요청을 승인해 주세요. 승인하기 전에는 아무것도 바뀌지 않아요."
        );
        assert_eq!(
            message_ko(&json!({"view":"action_request","status":"REJECTED","summary":{"title":"멤버 초대"}})).unwrap(),
            "요청을 거절했어요. 멤버 초대는 진행되지 않아요."
        );
        assert_eq!(josa("발매 신청 취소", "을", "를"), "발매 신청 취소를");
        assert_eq!(
            josa("AUDENIQ 계약 종료 예약", "은", "는"),
            "AUDENIQ 계약 종료 예약은"
        );
        assert_eq!(josa("지급 요청", "을", "를"), "지급 요청을");
        assert_eq!(
            message_ko(&json!({"view":"validation","ready_to_submit":false,"blocking":[1,2]}))
                .unwrap(),
            "신청 전에 채울 항목이 2개 남았어요."
        );
        assert!(message_ko(&json!({"view":"releases"})).is_none());
        // Every sentence is polite 해요체 and never says 오디닉/오데닉.
        for v in [
            json!({"view":"upload_status","state":"READY"}),
            json!({"view":"permission_request","status":"PENDING"}),
            json!({"view":"submitted"}),
        ] {
            let m = message_ko(&v).unwrap();
            assert!(m.ends_with("요."), "{m}");
            assert!(!m.contains("오디닉") && !m.contains("오데닉"));
        }
    }
}
