//! AUDENIQ's own Korean wording for common tool outcomes (`message_ko`).
//!
//! Assistants answer in their own words, so the same result reads differently
//! in ChatGPT, Claude or Grok. Each result whose meaning fits one sentence
//! carries the sentence AUDENIQ would say, in its voice (해요체, active voice
//! for what was done, no hype), and the server instructions ask Korean
//! replies to start from it. Amounts, declarations and review results are
//! never paraphrased here; those stay in the structured fields.
use serde_json::Value;

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The sentence for a tool result, or `None` when the result needs the
/// assistant to explain details (lists, previews, reports).
pub fn message_ko(v: &Value) -> Option<String> {
    let text = match s(v, "view") {
        "permission_request" => match s(v, "status") {
            "PENDING" => {
                "어데닉 스튜디오에 권한 요청을 보냈어요. 아래 링크에서 승인해 주세요.".into()
            }
            "APPROVED" => "권한이 더해졌어요. 이어서 진행할게요.".into(),
            "ALREADY_GRANTED" => "이미 필요한 권한이 있어요.".into(),
            "REJECTED" => "권한 요청을 거절했어요. 지금 권한으로 할 수 있는 일만 할게요.".into(),
            "EXPIRED" => "권한 요청 시간이 지났어요. 필요하면 다시 요청할게요.".into(),
            "WITHDRAWN" => "권한 요청을 거둬들였어요.".into(),
            _ => return None,
        },
        "action_request" => {
            let title = v["summary"]["title"].as_str().unwrap_or("요청한 작업");
            match s(v, "status") {
                "PENDING" => format!(
                    "'{title}'을(를) 어데닉 스튜디오에서 승인해 주세요. 승인하기 전에는 아무것도 바뀌지 않아요."
                ),
                "DONE" => format!("'{title}'을(를) 마쳤어요."),
                "FAILED" => format!("'{title}'을(를) 승인했지만 처리하지 못했어요."),
                "REJECTED" => format!("'{title}' 요청을 거절했어요. 아무것도 바뀌지 않았어요."),
                "EXPIRED" => format!("'{title}' 요청 시간이 지났어요. 필요하면 다시 요청할게요."),
                "WITHDRAWN" => format!("'{title}' 요청을 거둬들였어요. 아무것도 바뀌지 않았어요."),
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
            "WITHDRAWN" => "승인 대기 중이던 신청을 거둬들였어요. 접수되지 않았어요.".into(),
            "PENDING" | "" => {
                "신청서를 보냈어요. 어데닉 스튜디오에서 한 번 더 승인하면 접수돼요.".into()
            }
            _ => return None,
        },
        "submitted" => "발매를 신청했어요. 어데닉이 검토를 시작해요.".into(),
        "release_deleted" => "초안을 삭제했어요.".into(),
        "notifications_read" => "알림을 읽음으로 표시했어요.".into(),
        "document_proof" => "증빙 파일을 붙였어요. 어데닉이 확인할게요.".into(),
        "special_options" => "특수 콘텐츠 정보를 저장했어요.".into(),
        "split_invite_resent" => "분배 수락 메일을 다시 보냈어요.".into(),
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

    #[test]
    fn outcomes_read_in_audeniq_voice() {
        assert_eq!(
            message_ko(&json!({"view":"action_request","status":"PENDING","summary":{"title":"서비스 중단 요청"}})).unwrap(),
            "'서비스 중단 요청'을(를) 어데닉 스튜디오에서 승인해 주세요. 승인하기 전에는 아무것도 바뀌지 않아요."
        );
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
