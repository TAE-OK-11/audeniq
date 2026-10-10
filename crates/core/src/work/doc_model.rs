//! Document content model. Documents are stored as ProseMirror/Tiptap JSON,
//! never as HTML. Every save is validated here against an allow-list of node
//! types, marks and attributes, so a crafted API call cannot store script
//! URLs, raw HTML or unknown nodes; the editor renders only these nodes.
//!
//! The validator also derives what the server needs from the content: the
//! plain text (search, version compare), the attachments it shows (their
//! downloads follow the document's ACL) and the AUDENIQ entities it embeds
//! (kept as `work.links` so cards and records show their documents).
use crate::error::{Error, Result};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_DEPTH: usize = 40;
const MAX_NODES: usize = 60_000;
const MAX_TEXT: usize = 200_000;

/// Entity kinds a document may embed (see `entities::KINDS`).
pub const EMBED_KINDS: &[&str] = &[
    "release",
    "track",
    "artist",
    "label",
    "dsp",
    "inquiry",
    "task",
    "doc",
    "approval",
    "audit",
    "relationship",
    "document",
    "settlement_report",
    "payout",
];
/// Views of a release embed (/qc, /rights, /distribution).
pub const EMBED_VIEWS: &[&str] = &["summary", "qc", "rights", "distribution"];

#[derive(Debug, Default)]
pub struct Derived {
    pub text: String,
    pub attachments: BTreeSet<Uuid>,
    /// (kind, id) of embedded entities, without the audit pseudo-kind
    pub embeds: BTreeSet<(String, String)>,
    pub mentions: BTreeSet<Uuid>,
}

fn bad(what: &'static str) -> Error {
    Error::InvalidCode(what)
}

fn obj(v: &Value) -> Result<&Map<String, Value>> {
    v.as_object().ok_or(bad("DOC_CONTENT_INVALID"))
}

/// Only these keys may appear in a node's or mark's `attrs`.
fn attrs<'a>(
    m: &'a Map<String, Value>,
    allowed: &[&str],
) -> Result<Option<&'a Map<String, Value>>> {
    let Some(a) = m.get("attrs") else {
        return Ok(None);
    };
    if a.is_null() {
        return Ok(None);
    }
    let a = obj(a)?;
    if a.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(bad("DOC_ATTR_INVALID"));
    }
    Ok(Some(a))
}

fn short_str(v: Option<&Value>, max: usize) -> Result<Option<String>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s))
            if s.chars().count() <= max && !s.chars().any(|c| c.is_control() && c != '\n') =>
        {
            Ok(Some(s.clone()))
        }
        _ => Err(bad("DOC_ATTR_INVALID")),
    }
}

/// http(s) or mailto only — never javascript:, data:, vbscript: or relative tricks.
pub fn safe_url(s: &str) -> bool {
    let t = s.trim();
    if t.len() > 2048 || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && url::Url::parse(t).is_ok_and(|u| u.host_str().is_some())
        || (lower.starts_with("mailto:") && t.len() > 7 && t.contains('@'))
}

fn uuid_attr(v: Option<&Value>) -> Result<Uuid> {
    v.and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(bad("DOC_ATTR_INVALID"))
}

struct Walker {
    nodes: usize,
    out: Derived,
}

impl Walker {
    fn marks(&self, m: &Map<String, Value>) -> Result<()> {
        let Some(marks) = m.get("marks") else {
            return Ok(());
        };
        let marks = marks.as_array().ok_or(bad("DOC_CONTENT_INVALID"))?;
        if marks.len() > 8 {
            return Err(bad("DOC_CONTENT_INVALID"));
        }
        for mark in marks {
            let mk = obj(mark)?;
            match mk.get("type").and_then(Value::as_str) {
                Some("bold" | "italic" | "strike" | "code" | "underline") => {
                    attrs(mk, &[])?;
                }
                Some("link") => {
                    let a = attrs(mk, &["href", "target", "rel", "class"])?
                        .ok_or(bad("DOC_LINK_INVALID"))?;
                    let href = a.get("href").and_then(Value::as_str).unwrap_or("");
                    if !safe_url(href) {
                        return Err(bad("DOC_LINK_INVALID"));
                    }
                }
                _ => return Err(bad("DOC_MARK_INVALID")),
            }
        }
        Ok(())
    }

    fn children(&mut self, m: &Map<String, Value>, depth: usize) -> Result<()> {
        if let Some(c) = m.get("content") {
            let c = c.as_array().ok_or(bad("DOC_CONTENT_INVALID"))?;
            for child in c {
                self.node(child, depth + 1)?;
            }
        }
        Ok(())
    }

    fn block_end(&mut self) {
        if !self.out.text.ends_with('\n') && !self.out.text.is_empty() {
            self.out.text.push('\n');
        }
    }

    fn node(&mut self, v: &Value, depth: usize) -> Result<()> {
        self.nodes += 1;
        if self.nodes > MAX_NODES || depth > MAX_DEPTH {
            return Err(bad("DOC_TOO_LARGE"));
        }
        let m = obj(v)?;
        if m.keys()
            .any(|k| !matches!(k.as_str(), "type" | "attrs" | "content" | "marks" | "text"))
        {
            return Err(bad("DOC_CONTENT_INVALID"));
        }
        let ty = m
            .get("type")
            .and_then(Value::as_str)
            .ok_or(bad("DOC_CONTENT_INVALID"))?;
        if ty != "text" && m.contains_key("marks") {
            return Err(bad("DOC_CONTENT_INVALID"));
        }
        match ty {
            "text" => {
                let s = m
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(bad("DOC_CONTENT_INVALID"))?;
                if s.is_empty() || m.contains_key("content") {
                    return Err(bad("DOC_CONTENT_INVALID"));
                }
                self.marks(m)?;
                attrs(m, &[])?;
                if self.out.text.len() + s.len() > MAX_TEXT * 4 {
                    return Err(bad("DOC_TOO_LARGE"));
                }
                self.out.text.push_str(s);
                return Ok(());
            }
            "doc" if depth == 0 => {
                attrs(m, &[])?;
            }
            "paragraph" | "blockquote" | "bulletList" | "listItem" | "taskList" | "tableRow"
            | "detailsContent" | "detailsSummary" => {
                attrs(m, &["textAlign"])?;
            }
            "heading" => {
                let a = attrs(m, &["level", "textAlign"])?;
                let level = a
                    .and_then(|a| a.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                if !(1..=3).contains(&level) {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "orderedList" => {
                let a = attrs(m, &["start", "type"])?;
                if let Some(s) = a.and_then(|a| a.get("start"))
                    && !s.is_null()
                    && !s.as_u64().is_some_and(|n| n < 100_000)
                {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "taskItem" => {
                let a = attrs(m, &["checked"])?;
                if let Some(c) = a.and_then(|a| a.get("checked"))
                    && !c.is_boolean()
                {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "codeBlock" => {
                let a = attrs(m, &["language"])?;
                if let Some(l) = short_str(a.and_then(|a| a.get("language")), 30)?
                    && !l
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-+#_.".contains(c))
                {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "horizontalRule" | "hardBreak" | "table" => {
                attrs(m, &[])?;
                if ty == "hardBreak" {
                    self.out.text.push('\n');
                }
            }
            "tableHeader" | "tableCell" => {
                let a = attrs(m, &["colspan", "rowspan", "colwidth"])?;
                if let Some(a) = a {
                    for k in ["colspan", "rowspan"] {
                        if let Some(v) = a.get(k)
                            && !v.is_null()
                            && !v.as_u64().is_some_and(|n| (1..=50).contains(&n))
                        {
                            return Err(bad("DOC_ATTR_INVALID"));
                        }
                    }
                    if let Some(w) = a.get("colwidth")
                        && !w.is_null()
                        && !w.as_array().is_some_and(|ws| {
                            ws.len() <= 50
                                && ws.iter().all(|x| x.as_u64().is_some_and(|n| n <= 5000))
                        })
                    {
                        return Err(bad("DOC_ATTR_INVALID"));
                    }
                }
            }
            "details" => {
                let a = attrs(m, &["open"])?;
                if let Some(o) = a.and_then(|a| a.get("open"))
                    && !o.is_boolean()
                {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "callout" => {
                let a = attrs(m, &["tone"])?;
                let tone = a
                    .and_then(|a| a.get("tone"))
                    .and_then(Value::as_str)
                    .unwrap_or("info");
                if !matches!(tone, "info" | "warning" | "danger" | "success") {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "docImage" => {
                let a =
                    attrs(m, &["attachmentId", "alt", "width"])?.ok_or(bad("DOC_ATTR_INVALID"))?;
                self.out
                    .attachments
                    .insert(uuid_attr(a.get("attachmentId"))?);
                if let Some(alt) = short_str(a.get("alt"), 300)? {
                    self.out.text.push_str(&alt);
                }
                if let Some(w) = a.get("width")
                    && !w.is_null()
                    && !w.as_u64().is_some_and(|n| (40..=2000).contains(&n))
                {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "fileAttachment" => {
                let a =
                    attrs(m, &["attachmentId", "name", "size"])?.ok_or(bad("DOC_ATTR_INVALID"))?;
                self.out
                    .attachments
                    .insert(uuid_attr(a.get("attachmentId"))?);
                if let Some(n) = short_str(a.get("name"), 200)? {
                    self.out.text.push_str(&n);
                }
                if let Some(sz) = a.get("size")
                    && !sz.is_null()
                    && !sz.is_u64()
                {
                    return Err(bad("DOC_ATTR_INVALID"));
                }
            }
            "linkPreview" => {
                let a =
                    attrs(m, &["url", "title", "description"])?.ok_or(bad("DOC_ATTR_INVALID"))?;
                let url = a.get("url").and_then(Value::as_str).unwrap_or("");
                if !safe_url(url) || url.to_ascii_lowercase().starts_with("mailto:") {
                    return Err(bad("DOC_LINK_INVALID"));
                }
                for k in ["title", "description"] {
                    if let Some(s) = short_str(a.get(k), 300)? {
                        self.out.text.push_str(&s);
                        self.out.text.push(' ');
                    }
                }
                self.out.text.push_str(url);
            }
            "entityEmbed" => {
                let a =
                    attrs(m, &["kind", "id", "view", "label"])?.ok_or(bad("DOC_ATTR_INVALID"))?;
                let kind = a.get("kind").and_then(Value::as_str).unwrap_or("");
                if !EMBED_KINDS.contains(&kind) {
                    return Err(bad("DOC_EMBED_INVALID"));
                }
                let id = a.get("id").and_then(Value::as_str).unwrap_or("");
                let id_ok = match kind {
                    "dsp" => {
                        id.starts_with("D-")
                            && id.len() <= 5
                            && id.len() > 2
                            && id[2..].chars().all(|c| c.is_ascii_digit())
                    }
                    _ => Uuid::parse_str(id).is_ok(),
                };
                if !id_ok {
                    return Err(bad("DOC_EMBED_INVALID"));
                }
                if let Some(view) = short_str(a.get("view"), 20)?
                    && view != "summary"
                    && !(kind == "release" && EMBED_VIEWS.contains(&view.as_str()))
                {
                    return Err(bad("DOC_EMBED_INVALID"));
                }
                // label: a small snapshot shown while the live data loads (and searched)
                if let Some(l) = short_str(a.get("label"), 200)? {
                    self.out.text.push_str(&l);
                }
                if kind != "audit" {
                    self.out.embeds.insert((kind.to_string(), id.to_string()));
                }
            }
            "mention" => {
                let a = attrs(m, &["kind", "id", "label"])?.ok_or(bad("DOC_ATTR_INVALID"))?;
                let kind = a.get("kind").and_then(Value::as_str).unwrap_or("");
                let id = uuid_attr(a.get("id"))?;
                if let Some(l) = short_str(a.get("label"), 120)? {
                    self.out.text.push('@');
                    self.out.text.push_str(&l);
                }
                match kind {
                    "user" => {
                        self.out.mentions.insert(id);
                    }
                    "doc" => {
                        self.out.embeds.insert(("doc".into(), id.to_string()));
                    }
                    _ => return Err(bad("DOC_ATTR_INVALID")),
                }
            }
            _ => return Err(bad("DOC_NODE_INVALID")),
        }
        if matches!(ty, "text" | "hardBreak") {
            return Ok(());
        }
        self.children(m, depth)?;
        if !matches!(ty, "doc" | "mention") {
            self.block_end();
        }
        Ok(())
    }
}

/// Validate a document body and derive text, attachments, embeds and mentions.
pub fn validate(content: &Value) -> Result<Derived> {
    let size = serde_json::to_vec(content)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if size > MAX_BYTES {
        return Err(bad("DOC_TOO_LARGE"));
    }
    let m = obj(content)?;
    if m.get("type").and_then(Value::as_str) != Some("doc") {
        return Err(bad("DOC_CONTENT_INVALID"));
    }
    let mut w = Walker {
        nodes: 0,
        out: Derived::default(),
    };
    w.node(content, 0)?;
    let mut text = w.out.text.trim().to_string();
    if text.chars().count() > MAX_TEXT {
        text = text.chars().take(MAX_TEXT).collect();
    }
    w.out.text = text;
    if w.out.attachments.len() > 200 || w.out.embeds.len() > 300 || w.out.mentions.len() > 100 {
        return Err(bad("DOC_TOO_LARGE"));
    }
    Ok(w.out)
}

pub fn empty() -> Value {
    json!({"type": "doc", "content": []})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(content: Value) -> Value {
        json!({"type": "doc", "content": content})
    }

    #[test]
    fn accepts_the_editor_blocks_and_derives_text_and_refs() {
        let att = Uuid::new_v4();
        let rel = Uuid::new_v4();
        let user = Uuid::new_v4();
        let d = validate(&doc(json!([
            {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "장애"}]},
            {"type": "paragraph", "content": [
                {"type": "text", "text": "보기 ", "marks": [{"type": "bold"}]},
                {"type": "text", "text": "링크", "marks": [{"type": "link", "attrs": {"href": "https://audeniq.com/a", "target": "_blank"}}]},
                {"type": "mention", "attrs": {"kind": "user", "id": user.to_string(), "label": "kim"}}
            ]},
            {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": true}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "확인"}]}]}]},
            {"type": "callout", "attrs": {"tone": "warning"}, "content": [{"type": "paragraph"}]},
            {"type": "details", "attrs": {"open": false}, "content": [
                {"type": "detailsSummary", "content": [{"type": "text", "text": "더 보기"}]},
                {"type": "detailsContent", "content": [{"type": "paragraph"}]}
            ]},
            {"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableHeader", "attrs": {"colspan": 1, "rowspan": 1, "colwidth": null}, "content": [{"type": "paragraph"}]}]}]},
            {"type": "docImage", "attrs": {"attachmentId": att.to_string(), "alt": "캡처"}},
            {"type": "entityEmbed", "attrs": {"kind": "release", "id": rel.to_string(), "view": "qc", "label": "Paper Moon"}},
            {"type": "entityEmbed", "attrs": {"kind": "dsp", "id": "D-5"}},
            {"type": "linkPreview", "attrs": {"url": "https://example.com/x", "title": "가이드"}},
            {"type": "codeBlock", "attrs": {"language": "rust"}, "content": [{"type": "text", "text": "fn main() {}"}]},
            {"type": "horizontalRule"}
        ])))
        .unwrap();
        assert!(
            d.text.contains("장애")
                && d.text.contains("보기 링크@kim")
                && d.text.contains("Paper Moon")
        );
        assert!(d.attachments.contains(&att));
        assert!(
            d.embeds.contains(&("release".into(), rel.to_string()))
                && d.embeds.contains(&("dsp".into(), "D-5".into()))
        );
        assert!(d.mentions.contains(&user));
    }

    #[test]
    fn rejects_script_urls_html_and_unknown_nodes() {
        let link = |href: &str| {
            doc(
                json!([{"type": "paragraph", "content": [{"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": href}}]}]}]),
            )
        };
        for href in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            " javascript:x",
            "data:text/html,<script>",
            "vbscript:x",
            "//evil.com",
            "/relative",
        ] {
            assert!(validate(&link(href)).is_err(), "{href}");
        }
        assert!(validate(&link("mailto:ops@audeniq.com")).is_ok());
        // raw HTML node, unknown attrs, unknown marks, bad embed kinds and ids
        assert!(
            validate(&doc(
                json!([{"type": "html", "attrs": {"html": "<img onerror=x>"}}])
            ))
            .is_err()
        );
        assert!(
            validate(&doc(
                json!([{"type": "paragraph", "attrs": {"onclick": "x"}}])
            ))
            .is_err()
        );
        assert!(validate(&doc(json!([{"type": "paragraph", "content": [{"type": "text", "text": "x", "marks": [{"type": "textStyle", "attrs": {"style": "x"}}]}]}]))).is_err());
        assert!(validate(&doc(json!([{"type": "entityEmbed", "attrs": {"kind": "user", "id": Uuid::new_v4().to_string()}}]))).is_err());
        assert!(
            validate(&doc(
                json!([{"type": "entityEmbed", "attrs": {"kind": "release", "id": "1 OR 1=1"}}])
            ))
            .is_err()
        );
        assert!(
            validate(&doc(
                json!([{"type": "linkPreview", "attrs": {"url": "javascript:alert(1)"}}])
            ))
            .is_err()
        );
        assert!(
            validate(&doc(
                json!([{"type": "docImage", "attrs": {"attachmentId": "https://evil/x.png"}}])
            ))
            .is_err()
        );
        assert!(validate(&doc(json!([{"type": "heading", "attrs": {"level": 6}}]))).is_err());
        assert!(validate(&json!({"type": "paragraph"})).is_err());
        // depth bomb
        let mut v = json!({"type": "paragraph"});
        for _ in 0..50 {
            v = json!({"type": "blockquote", "content": [v]});
        }
        assert!(validate(&doc(json!([v]))).is_err());
    }
}
