//! MCP prompts: ready-made starting points the client can offer the artist
//! (Claude shows them as slash commands). Each one only fills the artist's
//! first message; every action still goes through the tools and their
//! confirmations. Written in AUDENIQ's Korean voice (AUDENIQ in English letters, 해요체); the
//! assistant answers in the artist's own language.
use serde_json::{Value, json};

struct Prompt {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// (name, description, required)
    arguments: &'static [(&'static str, &'static str, bool)],
    text: fn(&Value) -> String,
}

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// User-supplied values are quoted as data, never spliced in as instructions.
fn quoted(v: &str) -> String {
    let clean: String = v.chars().filter(|c| !c.is_control()).take(200).collect();
    format!("「{clean}」")
}

const PROMPTS: &[Prompt] = &[
    Prompt {
        name: "prepare_release",
        title: "새 발매 준비",
        description: "AUDENIQ에서 새 발매 초안을 만들고, 빠진 정보를 하나씩 채워요.",
        arguments: &[("title", "발매 제목 (선택)", false)],
        text: |a| {
            let title = arg(a, "title")
                .map(|t| format!(" 제목은 {}이에요.", quoted(t)))
                .unwrap_or_default();
            format!(
                "AUDENIQ에서 새 발매를 준비하고 싶어요.{title} 발매 요건을 먼저 확인하고, 초안을 만든 뒤 필요한 정보를 하나씩 물어봐 주세요. 음원과 커버는 AUDENIQ Studio에서 올릴게요."
            )
        },
    },
    Prompt {
        name: "check_release",
        title: "발매 점검",
        description: "발매 초안에서 신청 전에 고칠 점과 빠진 파일을 확인해요.",
        arguments: &[("release", "발매 제목 (선택)", false)],
        text: |a| {
            let which = arg(a, "release")
                .map(|t| format!("{} 발매를", quoted(t)))
                .unwrap_or_else(|| "작업 중인 발매를".into());
            format!(
                "{which} 점검해 주세요. 신청 전에 고칠 점과 빠진 파일을 알려 주고, 고칠 수 있는 건 어떻게 고칠지 먼저 보여 주세요."
            )
        },
    },
    Prompt {
        name: "submission_status",
        title: "신청 진행 상황",
        description: "신청한 발매의 검토 상태와 다음 할 일을 확인해요.",
        arguments: &[],
        text: |_| {
            "신청한 발매들의 검토 상태를 알려 주세요. 고칠 내용이나 내가 해야 할 일이 있으면 같이 알려 주세요.".into()
        },
    },
    Prompt {
        name: "settlement_check",
        title: "정산 확인",
        description: "정산 잔액과 최근 정산 내역, 지급 신청 현황을 확인해요.",
        arguments: &[],
        text: |_| {
            "AUDENIQ 정산 잔액과 최근 정산 내역, 지급 신청 현황을 정리해 주세요. 금액은 받은 그대로 원 단위로 보여 주세요.".into()
        },
    },
    Prompt {
        name: "artist_pages",
        title: "아티스트 페이지 정리",
        description: "플랫폼별 아티스트 페이지 링크를 확인하고 빠진 링크를 등록해요.",
        arguments: &[],
        text: |_| {
            "내 아티스트 페이지가 플랫폼마다 제대로 등록돼 있는지 확인해 주세요. 링크가 빠진 곳이 있으면 알려 주고, 내가 링크를 주면 등록하기 전에 한 번 보여 주세요.".into()
        },
    },
];

pub fn list_json() -> Vec<Value> {
    PROMPTS
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "title": p.title,
                "description": p.description,
                "arguments": p.arguments.iter().map(|(n, d, r)| json!({"name": n, "description": d, "required": r})).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// `prompts/get`: the messages for one prompt, or `None` if it is unknown.
pub fn get(name: &str, args: &Value) -> Option<Value> {
    let p = PROMPTS.iter().find(|p| p.name == name)?;
    Some(json!({
        "description": p.description,
        "messages": [{"role": "user", "content": {"type": "text", "text": (p.text)(args)}}],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prompts_are_listed_and_quote_arguments() {
        assert_eq!(list_json().len(), PROMPTS.len());
        let v = get("prepare_release", &json!({"title": "봄\u{0}노래"})).unwrap();
        let text = v["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("「봄노래」"), "{text}");
        assert!(text.contains("AUDENIQ에서"));
        assert!(get("nope", &json!({})).is_none());
        for p in PROMPTS {
            let t = (p.text)(&json!({}));
            assert!(!t.contains("오디닉") && !t.contains("오데닉"));
            assert!(!t.contains("하십시오") && !t.contains("시겠어요"));
        }
    }
}
