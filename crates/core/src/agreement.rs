//! 배급 계약서 (AUD-DIST 2.0): 릴리즈마다 AUDENIQ와 이용자가 맺는 개별 배급 계약.
//!
//! - Staff enter the commercial terms on the review sheet
//!   (`PUT /api/staff/releases/{id}/agreement-terms`): exclusivity, the
//!   AUDENIQ fee, special/promotional/route rates, territory exclusions,
//!   special terms. The server renders the agreement body from the release,
//!   the party (organisation, signer, payout account) and those terms.
//! - Approving a release's agreement requires the terms
//!   (`AGREEMENT_TERMS_REQUIRED`, staff.rs).
//! - The artist signs with every required rights confirmation ticked
//!   (`portal::sign_document`, `AGREEMENT_CONFIRMATION_REQUIRED`). The
//!   confirmations depend on the release (co-owners, AI, minor).
//!
//! Common service rules (settlement cycle, holds, takedown, disputes …) stay in
//! the terms of service; the agreement only states what the parties agree on
//! for this release and points to the terms for the rest.
use crate::{
    api::AppState,
    domain::digest,
    dsp_registry,
    error::{Error, Result},
    operations,
    staff::{self, Duty},
    text_policy,
};
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub const FORM: &str = "AUD-DIST 2.0";
pub const TERMS_TITLE: &str = "AUDENIQ 음원 배급 서비스 이용약관";
pub const TERMS_VERSION: &str = "AUD-TERMS 2026.10";

/// One box the artist ticks before signing. `required` boxes must all be
/// ticked; optional ones are recorded as given (e.g. settlement authority).
pub struct Confirmation {
    pub id: &'static str,
    pub text: &'static str,
    pub required: bool,
}

const BASE: [Confirmation; 8] = [
    Confirmation {
        id: "grant",
        required: true,
        text: "이 계약의 대상 콘텐츠를 저장·복제·전송하고, DSP에 전달하며, DSP와 배급 파트너가 배급에 필요한 범위에서 이용하게 하고, 메타데이터·앨범아트를 전달하고, 배급 관련 행정업무를 수행할 권한을 회사에 부여합니다.",
    },
    Confirmation {
        id: "rights_own",
        required: true,
        text: "배급에 필요한 권리를 직접 보유하거나, 권리자로부터 필요한 허락을 받았습니다.",
    },
    Confirmation {
        id: "rights_scope",
        required: true,
        text: "저작권(작사·작곡), 저작인접권, 실연자의 권리, 마스터(음반제작자)의 권리를 확인했습니다.",
    },
    Confirmation {
        id: "third_party",
        required: true,
        text: "샘플·비트·앨범아트·사진·이미지·영상 등 제3자 자료를 쓸 권리를 확보했습니다.",
    },
    Confirmation {
        id: "coauthors",
        required: true,
        text: "다른 사람과 함께 만든 콘텐츠라면 필요한 동의와 위임을 받았습니다.",
    },
    Confirmation {
        id: "documents_true",
        required: true,
        text: "제출한 위임서·계약서·라이선스 등 자료는 모두 진실합니다.",
    },
    Confirmation {
        id: "no_misuse",
        required: true,
        text: "타인의 명의나 콘텐츠를 허락 없이 사용하지 않았습니다.",
    },
    Confirmation {
        id: "terms",
        required: true,
        text: "AUDENIQ 음원 배급 서비스 이용약관을 확인했고, 이 계약에서 정하지 않은 사항에 약관이 적용되는 데 동의합니다.",
    },
];
const SHARED: [Confirmation; 2] = [
    Confirmation {
        id: "representative",
        required: true,
        text: "공동 권리자들로부터 위임받은 대표권리자로서, 위임받은 범위(배급 신청·계약 체결·수정·테이크다운) 안에서 이 계약을 체결합니다.",
    },
    Confirmation {
        id: "settlement_authority",
        required: false,
        text: "공동 권리자들로부터 정산금을 대표로 받을 권한까지 위임받았습니다.",
    },
];
const AI: [Confirmation; 2] = [
    Confirmation {
        id: "ai_commercial",
        required: true,
        text: "사용한 AI 서비스의 상업적 이용·배급 조건을 확인했고, 출력물을 배급할 권리가 있습니다.",
    },
    Confirmation {
        id: "ai_voice",
        required: true,
        text: "보이스 클론 등으로 다른 사람의 목소리·초상·이름을 허락 없이 사용하지 않았습니다.",
    },
];
const MINOR: Confirmation = Confirmation {
    id: "guardian",
    required: true,
    text: "법정대리인으로서 미성년 아티스트의 이 계약 체결에 동의합니다.",
};

/// The boxes for a release, from its declared options.
pub fn confirmations(options: &Value) -> Vec<&'static Confirmation> {
    let mut v: Vec<&'static Confirmation> = BASE.iter().collect();
    if options["shared"] == true {
        v.extend(SHARED.iter());
    }
    if options["ai"] == true {
        v.extend(AI.iter());
    }
    if options["minor"] == true {
        v.push(&MINOR);
    }
    v
}

/// Checks the artist's ticks against the stored terms; returns the record to
/// keep with the signature.
pub fn check_confirmations(terms: &Value, ticked: &[String]) -> Result<Value> {
    let items: Vec<Value> = terms["confirmations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if ticked.len() > 20
        || ticked
            .iter()
            .any(|t| !items.iter().any(|i| i["id"] == t.as_str()))
    {
        return Err(Error::Invalid);
    }
    let missing = items
        .iter()
        .any(|i| i["required"] == true && !ticked.iter().any(|t| i["id"] == t.as_str()));
    if missing {
        return Err(Error::PolicyGate("AGREEMENT_CONFIRMATION_REQUIRED"));
    }
    Ok(json!({
        "items": items.iter().map(|i| json!({
            "id": i["id"], "text": i["text"], "required": i["required"],
            "checked": ticked.iter().any(|t| i["id"] == t.as_str()),
        })).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------
// Terms entered by staff
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermsInput {
    pub exclusivity: String,
    /// AUDENIQ share of the net receipts, basis points (800 = 8%).
    pub fee_bps: u32,
    #[serde(default)]
    pub rate_note: String,
    #[serde(default)]
    pub territory_note: String,
    #[serde(default)]
    pub min_payout_note: String,
    #[serde(default)]
    pub special_terms: String,
}

fn line(s: &str, max: usize) -> Result<String> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err(Error::Invalid);
    }
    text_policy::check_multiline(t)?;
    Ok(t.to_string())
}

fn pct(bps: u32) -> String {
    if bps.is_multiple_of(100) {
        format!("{}%", bps / 100)
    } else {
        let frac = format!("{:02}", bps % 100);
        format!("{}.{}%", bps / 100, frac.trim_end_matches('0'))
    }
}

struct Context {
    title: String,
    release_type: String,
    draft: Value,
    org_name: String,
    org_kind: String,
    payee_type: Option<String>,
    holder: Option<String>,
    application_no: Option<String>,
    signer: Option<String>,
    signer_role: Option<String>,
    tracks: Vec<(String, Option<String>)>,
    delegations: Vec<String>,
}

async fn context(c: &mut PgConnection, release: Uuid) -> Result<Context> {
    let r = sqlx::query(
        "SELECT r.title, r.release_type, r.draft, o.name AS org_name, o.kind AS org_kind,
                pa.payee_type, pa.holder_name, ra.application_no, ra.signer_name, ra.signer_role
         FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
         LEFT JOIN portal.payout_accounts pa ON pa.org_id=r.org_id
         LEFT JOIN LATERAL (SELECT application_no, signer_name, signer_role FROM portal.release_applications
                            WHERE release_id=r.id ORDER BY received_at DESC LIMIT 1) ra ON true
         WHERE r.id=$1",
    )
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    let tracks: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT title, isrc FROM catalog.tracks WHERE release_id=$1 ORDER BY disc_number, track_number",
    )
    .bind(release)
    .fetch_all(&mut *c)
    .await?;
    // Co-owner delegations on file for this release: signed electronic
    // documents by number, uploaded ones by file name.
    let delegations: Vec<String> = sqlx::query_scalar(
        "SELECT CASE WHEN electronic_record IS NOT NULL
                     THEN title || ' (전자문서 AUD-RIGHTS-' || (electronic_record->>'document_no') || ')'
                     ELSE title || ' (첨부 ' || NULLIF(file_name,'') || ')' END
         FROM portal.documents
         WHERE release_id=$1 AND kind='RIGHTS_PROOF' AND status NOT IN ('NEEDS')
           AND (electronic_record->>'document_kind'='shared' OR title LIKE '%위임%')
           AND (electronic_record IS NOT NULL OR file_name<>'')
         ORDER BY created_at",
    )
    .bind(release)
    .fetch_all(&mut *c)
    .await?;
    Ok(Context {
        title: r.get("title"),
        release_type: r.get("release_type"),
        draft: r.get("draft"),
        org_name: r.get("org_name"),
        org_kind: r.get("org_kind"),
        payee_type: r.get("payee_type"),
        holder: r.get("holder_name"),
        application_no: r.get("application_no"),
        signer: r.get("signer_name"),
        signer_role: r.get("signer_role"),
        tracks,
        delegations,
    })
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().map(str::trim).unwrap_or("")
}

fn party_kind(ctx: &Context) -> &'static str {
    match ctx.payee_type.as_deref() {
        Some("INDIVIDUAL") => "개인",
        Some("SOLE_PROPRIETOR") => "개인사업자",
        Some("CORPORATION") => "법인",
        _ => match ctx.org_kind.as_str() {
            "PERSONAL" => "개인",
            "LABEL" => "레이블",
            _ => "법인·단체",
        },
    }
}

/// The agreement text. Numbering follows the articles actually present.
fn render(ctx: &Context, t: &Value, items: &[&Confirmation]) -> String {
    let d = &ctx.draft;
    let o = &d["options"];
    let artist = if s(d, "artist").is_empty() {
        ctx.org_name.as_str()
    } else {
        s(d, "artist")
    };
    let kind = match ctx.release_type.as_str() {
        "SINGLE" => "싱글",
        "EP" => "EP",
        _ => "앨범",
    };
    let platforms: Vec<&str> = dsp_registry::requested(&json!({"platforms": d["platforms"]}))
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.display_name())
        .collect();
    let world = d["territories"]
        .as_array()
        .is_none_or(|a| a.is_empty() || a.iter().any(|x| x == "WORLD"));
    let note = |k: &str| t[k].as_str().unwrap_or("").trim().to_string();
    let fee = t["fee_bps"].as_u64().unwrap_or(0) as u32;
    let mut out: Vec<String> = vec![
        "AUDENIQ 음원 배급 계약서".into(),
        String::new(),
        format!("서식 {FORM} · 적용 약관 {TERMS_TITLE} ({TERMS_VERSION})"),
        format!(
            "신청서 번호 {}",
            ctx.application_no.as_deref().unwrap_or("미접수")
        ),
        String::new(),
        "주식회사 AUDENIQ(이하 “회사”)와 아래 이용자는 아래 릴리즈의 디지털 음원 배급에 관하여 다음과 같이 계약합니다.".into(),
    ];
    let mut n = 0;
    let mut art = |out: &mut Vec<String>, title: &str, body: Vec<String>| {
        n += 1;
        out.push(String::new());
        out.push(format!("제{n}조 ({title})"));
        out.extend(body);
    };
    let signer = ctx.signer.as_deref().unwrap_or(artist);
    let role = ctx.signer_role.as_deref().unwrap_or("아티스트 본인");
    let mut parties = vec![
        "회사: 주식회사 AUDENIQ".into(),
        format!(
            "이용자: {} ({}) · 아티스트 {artist}",
            ctx.org_name,
            party_kind(ctx)
        ),
        format!("서명자: {signer} ({role})"),
    ];
    if o["minor"] == true {
        let g = s(o, "guardian");
        let rel = s(o, "guardianRelation");
        parties.push(format!(
            "미성년 아티스트: 법정대리인 {}{}의 동의를 받아 체결합니다.",
            if g.is_empty() {
                "(신청서 기재)"
            } else {
                g
            },
            if rel.is_empty() {
                String::new()
            } else {
                format!("({rel})")
            }
        ));
    }
    art(&mut out, "계약 당사자", parties);
    art(
        &mut out,
        "계약 대상",
        vec![
            "이 계약은 아래 릴리즈 한 건에 적용합니다. 트랙 목록은 별첨 1과 같습니다.".into(),
            format!("발매명: {} ({kind})", ctx.title),
            format!("아티스트: {artist}"),
            format!(
                "UPC/EAN: {}",
                if s(d, "upc").is_empty() {
                    "회사 발급 예정"
                } else {
                    s(d, "upc")
                }
            ),
            format!(
                "발매 예정일: {}",
                if s(d, "release_date").is_empty() {
                    "신청서 기재일"
                } else {
                    s(d, "release_date")
                }
            ),
        ],
    );
    let exclusive = t["exclusivity"] == "EXCLUSIVE";
    art(
        &mut out,
        "배급 형태와 지역",
        vec![
            if exclusive {
                "배급 형태: 독점 — 이용자는 계약기간 동안 대상 콘텐츠를 다른 배급사를 통해 같은 DSP에 배급하지 않습니다.".into()
            } else {
                "배급 형태: 비독점 — 이용자는 대상 콘텐츠를 다른 경로로도 배급할 수 있습니다. 다만 같은 DSP에 중복 배급해 생긴 문제는 이용자가 해결합니다.".into()
            },
            format!(
                "배급 지역: {}{}",
                if world {
                    "전 세계"
                } else {
                    "신청서에 지정한 지역"
                },
                match note("territory_note") {
                    x if x.is_empty() => String::new(),
                    x => format!(" (제외·조건: {x})"),
                }
            ),
        ],
    );
    art(
        &mut out,
        "대상 DSP와 서비스 범위",
        vec![
            format!(
                "대상 DSP: {}",
                if platforms.is_empty() { "신청서에 지정한 DSP".into() } else { platforms.join(", ") }
            ),
            "서비스 범위: 기본 음원 배급. UGC 권리관리(YouTube Content ID, TikTok·Meta 등)와 그 밖의 부가서비스는 이 계약에 포함하지 않으며, 신청하는 경우 별도로 정합니다.".into(),
        ],
    );
    art(
        &mut out,
        "계약기간",
        vec!["이용자가 서명한 날부터 이용자가 해지를 요청할 때까지로 합니다. 해지 요청과 그 후의 처리는 약관 제18장(계약 종료)에 따릅니다.".into()],
    );
    let mut fee_body = vec![format!(
        "배급수수료: 회사가 DSP와 배급 파트너로부터 실제로 받은 대상 콘텐츠의 수익 중 회사 {} / 이용자 {}",
        pct(fee),
        pct(10_000 - fee)
    )];
    if !note("rate_note").is_empty() {
        fee_body.push(format!("특별 요율: {}", note("rate_note")));
    }
    art(&mut out, "배급수수료", fee_body);
    let payee = match (ctx.holder.as_deref(), ctx.payee_type.as_deref()) {
        (Some(h), Some(_)) => format!("{h} ({})", party_kind(ctx)),
        _ => "이용자가 서비스에 등록한 수령 계좌의 예금주".into(),
    };
    let mut settle = vec![
        format!("정산 수령인: {payee}"),
        "지급 통화: 대한민국 원(KRW)".into(),
    ];
    if !note("min_payout_note").is_empty() {
        settle.push(format!("최소지급액 특약: {}", note("min_payout_note")));
    }
    settle.push("정산 주기, 공제·조정, 환수, 지급 보류와 최소지급액의 일반 기준은 약관 제15장(정산)에 따릅니다.".into());
    art(&mut out, "정산", settle);
    art(
        &mut out,
        "개별 특약",
        vec![match note("special_terms") {
            x if x.is_empty() => "없음".into(),
            x => x,
        }],
    );
    art(
        &mut out,
        "배급 권한의 부여",
        vec!["이용자는 대상 콘텐츠에 관하여 약관 제11조부터 제13조까지에서 정한 범위의 이용허락과 업무 수행 권한을 회사에 부여하며, 서명할 때 별첨 2의 해당 항목에 직접 체크하여 다시 확인합니다. 이 계약으로 저작권 등 권리 자체가 회사에 양도되지 않습니다.".into()],
    );
    art(
        &mut out,
        "권리의 최종 확인",
        vec!["이용자는 서명할 때 별첨 2의 권리 확인 항목에 직접 체크합니다. 확인한 내용이 사실과 달라 생긴 손해는 약관 제93조에 따라 이용자가 책임집니다.".into()],
    );
    if o["shared"] == true {
        let mut body = vec!["대상 콘텐츠에는 공동 권리자가 있으며, 이용자는 대표권리자로서 이 계약을 체결합니다. 위임 범위와 정산금 수령 권한 여부는 별첨 2의 체크 결과에 따릅니다.".into()];
        body.push(if ctx.delegations.is_empty() {
            "위임 자료: 서비스에 제출한 공동 권리자 위임 서류(제출 전이면 회사가 요청할 수 있습니다)".into()
        } else {
            format!("위임 자료: {}", ctx.delegations.join(" / "))
        });
        art(&mut out, "대표권리자", body);
    }
    if o["ai"] == true {
        let tools: Vec<String> = o["aiTools"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let tools = if !tools.is_empty() {
            tools.join(", ")
        } else if !s(o, "aiTool").is_empty() {
            s(o, "aiTool").to_string()
        } else {
            "신청서 기재".into()
        };
        art(
            &mut out,
            "AI 사용",
            vec![
                format!("AI 사용 여부: 사용함 · 사용한 AI 서비스: {tools}"),
                "AI 콘텐츠에 관한 제한과 심사 기준은 약관 제9장에 따릅니다.".into(),
            ],
        );
    }
    art(
        &mut out,
        "약관의 적용",
        vec![format!(
            "이 계약에서 정하지 않은 사항은 {TERMS_TITLE}({TERMS_VERSION})에 따릅니다. 이 계약과 약관의 내용이 서로 다르면 이 계약을 우선합니다."
        )],
    );
    art(
        &mut out,
        "전자계약",
        vec!["이 계약은 전자문서로 작성하고 전자서명으로 체결합니다. 체결일은 이용자가 서명한 날이며, 회사는 담당자 승인으로 계약 내용을 확정한 뒤 이용자의 서명을 받습니다. 계약서 번호·버전·서명 기록은 함께 보관됩니다.".into()],
    );
    out.push(String::new());
    out.push("별첨 1 · 트랙 목록".into());
    if ctx.tracks.is_empty() {
        out.push("신청서에 기재한 트랙".into());
    }
    for (i, (title, isrc)) in ctx.tracks.iter().enumerate() {
        out.push(format!(
            "{}. {title}{}",
            i + 1,
            isrc.as_deref()
                .map(|x| format!(" (ISRC {x})"))
                .unwrap_or_default()
        ));
    }
    out.push(String::new());
    out.push("별첨 2 · 서명 시 확인 항목".into());
    for c in items {
        out.push(format!(
            "□ {}{}",
            if c.required {
                "(필수) "
            } else {
                "(해당 시) "
            },
            c.text
        ));
    }
    out.join("\n")
}

/// Staff set (or replace) the commercial terms of the release's agreement
/// while it waits on staff; the body is rendered from them.
pub async fn set_terms(s: &AppState, h: &HeaderMap, release: Uuid, i: TermsInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Review)?;
    if !matches!(i.exclusivity.as_str(), "NON_EXCLUSIVE" | "EXCLUSIVE") || i.fee_bps > 10_000 {
        return Err(Error::Invalid);
    }
    let terms_in = json!({
        "rate_note": line(&i.rate_note, 500)?,
        "territory_note": line(&i.territory_note, 300)?,
        "min_payout_note": line(&i.min_payout_note, 300)?,
        "special_terms": line(&i.special_terms, 2000)?,
    });
    let mut tx = s.pool.begin().await?;
    let doc: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, org_id FROM portal.documents
         WHERE release_id=$1 AND kind='AGREEMENT' AND status IN ('REVIEW','PREPARED') FOR UPDATE",
    )
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let (id, org) = doc.ok_or(Error::PolicyGate("AGREEMENT_NOT_IN_REVIEW"))?;
    let ctx = context(&mut tx, release).await?;
    let items = confirmations(&ctx.draft["options"]);
    let mut terms = json!({
        "form": FORM, "terms_version": TERMS_VERSION,
        "exclusivity": i.exclusivity, "fee_bps": i.fee_bps, "user_bps": 10_000 - i.fee_bps,
        "currency": "KRW",
        "confirmations": items.iter().map(|c| json!({"id": c.id, "text": c.text, "required": c.required})).collect::<Vec<_>>(),
        "required": items.iter().filter(|c| c.required).map(|c| c.id).collect::<Vec<_>>(),
        "set_by": st.actor.user,
    });
    for (k, v) in terms_in.as_object().unwrap() {
        terms[k] = v.clone();
    }
    let body = render(&ctx, &terms, &items);
    let title = format!(
        "{} · AUDENIQ 음원 배급 계약서",
        ctx.title.chars().take(120).collect::<String>()
    );
    let rv: i64 = sqlx::query_scalar(
        "UPDATE portal.documents SET title=$2, body=$3, agreement_terms=$4 || jsonb_build_object('set_at', now()),
           checked_at=NULL, row_version=row_version+1, updated_at=now()
         WHERE id=$1 RETURNING row_version",
    )
    .bind(id)
    .bind(&title)
    .bind(&body)
    .bind(&terms)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(id),
        "staff.agreement_terms_set",
        if i.exclusivity == "EXCLUSIVE" {
            "EXCLUSIVE"
        } else {
            "NON_EXCLUSIVE"
        },
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "row_version": rv, "terms": terms, "body": body}))
}

/// The hash kept with the signature: body, terms and the ticks.
pub fn content_hash(body: &str, terms: &Value, ticks: &Value, signature: &str) -> String {
    digest(
        &json!({"form": FORM, "body": body, "terms": terms, "confirmations": ticks, "signature": signature}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(options: Value) -> Context {
        Context {
            title: "첫 번째 싱글".into(),
            release_type: "SINGLE".into(),
            draft: json!({"artist": "서린", "upc": "8800000000011", "release_date": "2026-10-01",
                "territories": ["WORLD"], "platforms": ["spotify", "melon"], "options": options}),
            org_name: "서린".into(),
            org_kind: "PERSONAL".into(),
            payee_type: Some("INDIVIDUAL".into()),
            holder: Some("서린".into()),
            application_no: Some("AUD-20260920-ABCDEF".into()),
            signer: Some("서린".into()),
            signer_role: Some("아티스트 본인".into()),
            tracks: vec![("첫 번째 싱글".into(), Some("KRA262600001".into()))],
            delegations: vec![],
        }
    }

    #[test]
    fn renders_agreed_terms_and_points_to_the_terms_of_service() {
        let ctx = sample(json!({"shared": true, "ai": true, "aiTools": ["Suno"]}));
        let items = confirmations(&ctx.draft["options"]);
        let terms = json!({"exclusivity": "NON_EXCLUSIVE", "fee_bps": 800, "rate_note": "프로모션 요율 (2026년 12월까지)",
            "territory_note": "", "min_payout_note": "", "special_terms": ""});
        let body = render(&ctx, &terms, &items);
        if std::env::var("PRINT_AGREEMENT").is_ok() {
            println!("{body}");
        }
        for want in [
            "서식 AUD-DIST 2.0",
            "제1조 (계약 당사자)",
            "이용자: 서린 (개인)",
            "배급 형태: 비독점",
            "회사 8% / 이용자 92%",
            "특별 요율: 프로모션 요율",
            "Spotify",
            "개별 특약",
            "없음",
            "제11조 (대표권리자)",
            "AI 사용 여부: 사용함 · 사용한 AI 서비스: Suno",
            "계약과 약관의 내용이 서로 다르면 이 계약을 우선합니다",
            "1. 첫 번째 싱글 (ISRC KRA262600001)",
            "□ (해당 시) 공동 권리자들로부터 정산금을",
        ] {
            assert!(body.contains(want), "missing {want}\n{body}");
        }
        // Articles are numbered in order without gaps.
        let numbers: Vec<usize> = body
            .lines()
            .filter_map(|l| l.strip_prefix('제')?.split('조').next()?.parse().ok())
            .collect();
        assert_eq!(numbers, (1..=numbers.len()).collect::<Vec<_>>());
    }

    #[test]
    fn percentages() {
        assert_eq!(pct(800), "8%");
        assert_eq!(pct(9200), "92%");
        assert_eq!(pct(1250), "12.5%");
        assert_eq!(pct(5), "0.05%");
    }

    #[test]
    fn confirmations_follow_the_release() {
        let base = confirmations(&json!({}));
        assert_eq!(base.len(), 8);
        let all = confirmations(&json!({"shared": true, "ai": true, "minor": true}));
        let ids: Vec<&str> = all.iter().map(|c| c.id).collect();
        assert!(
            ids.contains(&"representative")
                && ids.contains(&"ai_voice")
                && ids.contains(&"guardian")
        );
        assert!(
            !all.iter()
                .find(|c| c.id == "settlement_authority")
                .unwrap()
                .required
        );
    }

    #[test]
    fn ticks_must_cover_required_boxes() {
        let items = confirmations(&json!({"shared": true}));
        let terms = json!({"confirmations": items.iter().map(|c| json!({"id": c.id, "text": c.text, "required": c.required})).collect::<Vec<_>>()});
        let required: Vec<String> = items
            .iter()
            .filter(|c| c.required)
            .map(|c| c.id.to_string())
            .collect();
        let ok = check_confirmations(&terms, &required).unwrap();
        let settle = ok["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == "settlement_authority")
            .unwrap();
        assert_eq!(settle["checked"], false);
        assert!(check_confirmations(&terms, &required[1..]).is_err());
        let mut extra = required.clone();
        extra.push("ai_voice".into());
        assert!(
            check_confirmations(&terms, &extra).is_err(),
            "boxes not in this agreement are refused"
        );
    }
}
