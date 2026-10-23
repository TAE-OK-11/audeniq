//! Actions an assistant may only ask for (migration 0144): the artist
//! approves each one once in AUDENIQ Studio, and approval runs it with the
//! artist's own session, exactly as if they had done it there.
//!
//! - Withdrawing an application, taking a release down, revenue splits,
//!   release and contract end dates, counter notices, member invites and
//!   payouts above the assistant limit.
//! - The tool checks what it can now (shape, access, amounts) and stores the
//!   payload with a Korean summary the approval page shows. The artist gets
//!   a notice with the link; the assistant polls `get_action_request`.
//! - Undoing a scheduled change (takedown, release end, contract end) and
//!   the special-content options of a draft only need the artist's explicit
//!   word in the conversation (`artist_confirmed`).
use super::{
    oauth::Principal,
    tools::{ToolError, tool_error},
};
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

type ToolResult = std::result::Result<Value, ToolError>;

/// The artist must answer a request within this many hours.
pub const REQUEST_HOURS: i32 = 24;
/// Requests one connection may file per rate window.
const REQUESTS_PER_WINDOW: i32 = 20;

/// The tools this module answers ([`call`]).
pub(super) const TOOLS: [&str; 15] = [
    "request_release_withdrawal",
    "request_takedown",
    "request_split_change",
    "request_release_end",
    "request_contract_end",
    "request_counter_notice",
    "request_member_invite",
    "request_large_payout",
    "get_action_request",
    "cancel_takedown",
    "cancel_release_end",
    "cancel_contract_end",
    "set_special_options",
    "withdraw_action_request",
    "withdraw_pending_submission",
];

pub(super) async fn call(s: &AppState, p: &Principal, name: &str, args: &Value) -> ToolResult {
    match name {
        "get_action_request" => status(s, p, args).await,
        "cancel_takedown" => {
            confirmed(args)?;
            let release = uuid_arg(args, "release_id")?;
            let v = crate::takedown::cancel(s, &p.actor, p.org, release).await?;
            Ok(view(v, "takedown"))
        }
        "cancel_release_end" => {
            confirmed(args)?;
            let release = uuid_arg(args, "release_id")?;
            let v = crate::contract_terms::cancel_release_term(s, &p.actor, p.org, release).await?;
            Ok(view(v, "release_end"))
        }
        "cancel_contract_end" => {
            confirmed(args)?;
            let v = crate::contract_terms::cancel_org_term(s, &p.actor, p.org).await?;
            Ok(view(v, "contract_end"))
        }
        "set_special_options" => Box::pin(special_options(s, p, args)).await,
        "withdraw_action_request" => withdraw(s, p, args).await,
        "withdraw_pending_submission" => withdraw_submission(s, p, args).await,
        _ => {
            let (action, release, payload, summary) = Box::pin(prepare(s, p, name, args)).await?;
            file(s, p, action, release, payload, summary).await
        }
    }
}

fn uuid_arg(args: &Value, key: &str) -> std::result::Result<Uuid, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| tool_error("INVALID_ARGUMENTS", &format!("{key} must be a UUID.")))
}

fn str_arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

fn confirmed(args: &Value) -> std::result::Result<(), ToolError> {
    if args["artist_confirmed"] == true {
        Ok(())
    } else {
        Err(tool_error(
            "ARTIST_CONFIRMATION_REQUIRED",
            "Ask the artist first and pass artist_confirmed: true only after they said yes.",
        ))
    }
}

fn view(mut v: Value, name: &str) -> Value {
    if !v.is_object() {
        v = json!({"result": v});
    }
    v["view"] = json!(name);
    v
}

fn line(label: &str, value: impl Into<String>) -> Value {
    json!({"label": label, "value": value.into()})
}

fn summary(title: &str, lines: Vec<Value>, warning: &str) -> Value {
    json!({"title": title, "lines": lines, "warning": warning})
}

/// The release's title, after checking the caller may change it.
async fn release_title(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<String> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let title: String =
        sqlx::query_scalar("SELECT title FROM catalog.releases WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(release)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(title)
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> std::result::Result<T, ToolError> {
    serde_json::from_value(v).map_err(|_| {
        tool_error(
            "INVALID_ARGUMENTS",
            "The request is missing or has invalid fields.",
        )
    })
}

fn won(v: &Value) -> String {
    let n = v.as_str().unwrap_or("0");
    let digits: String = n.split('.').next().unwrap_or("0").to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out}원")
}

fn masked_email(e: &str) -> String {
    match e.split_once('@') {
        Some((user, domain)) => {
            let first: String = user.chars().take(1).collect();
            format!("{first}***@{domain}")
        }
        None => "***".into(),
    }
}

/// Validates one request and builds what the approval page shows.
async fn prepare(
    s: &AppState,
    p: &Principal,
    name: &str,
    args: &Value,
) -> std::result::Result<(&'static str, Option<Uuid>, Value, Value), ToolError> {
    let a = &p.actor;
    Ok(match name {
        "request_release_withdrawal" => {
            let release = uuid_arg(args, "release_id")?;
            let title = release_title(s, a, p.org, release).await?;
            let status: String =
                sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
                    .bind(release)
                    .fetch_one(&s.pool)
                    .await
                    .map_err(Error::from)?;
            if status == "DRAFT" {
                return Err(tool_error(
                    "RELEASE_NOT_SUBMITTED",
                    "This release was never submitted. Delete the draft with delete_release_draft instead.",
                ));
            }
            let reason = str_arg(args, "reason");
            let mut lines = vec![line("발매", title)];
            if !reason.is_empty() {
                lines.push(line("사유", reason));
            }
            (
                "WITHDRAW",
                Some(release),
                json!({}),
                summary(
                    "발매 신청 취소",
                    lines,
                    "신청을 취소하면 심사가 멈추고 함께 낸 신청서도 취소돼요. 한 달에 취소할 수 있는 횟수가 정해져 있고, 플랫폼 전송이 시작된 뒤에는 취소할 수 없어요.",
                ),
            )
        }
        "request_takedown" => {
            let release = uuid_arg(args, "release_id")?;
            let title = release_title(s, a, p.org, release).await?;
            let payload = json!({"reason": str_arg(args, "reason")});
            let _: crate::takedown::RequestInput = parse(payload.clone())?;
            (
                "TAKEDOWN",
                Some(release),
                payload.clone(),
                summary(
                    "서비스 중단 요청",
                    vec![line("발매", title), line("사유", str_arg(args, "reason"))],
                    "AUDENIQ이 검토한 뒤 모든 플랫폼에서 이 발매를 내려요. 내려간 발매를 다시 올리려면 새로 신청해야 해요.",
                ),
            )
        }
        "request_split_change" => {
            let release = uuid_arg(args, "release_id")?;
            let title = release_title(s, a, p.org, release).await?;
            let payload = json!({"tracks": args["tracks"]});
            let sheet: crate::splits::SheetInput = parse(payload.clone())?;
            let names: std::collections::HashMap<Uuid, String> =
                sqlx::query_as::<_, (Uuid, String)>(
                    "SELECT id, title FROM catalog.tracks WHERE release_id=$1",
                )
                .bind(release)
                .fetch_all(&s.pool)
                .await
                .map_err(Error::from)?
                .into_iter()
                .collect();
            let mut lines = vec![line("발매", title)];
            for t in &sheet.tracks {
                let track = names.get(&t.track_id).ok_or_else(|| {
                    tool_error("INVALID_ARGUMENTS", "A track_id is not on this release.")
                })?;
                let people: Vec<String> = t
                    .shares
                    .iter()
                    .map(|sh| {
                        let pct = format!("{}.{:02}%", sh.share_bps / 100, sh.share_bps % 100);
                        match &sh.email {
                            Some(e) => format!("{} {} ({})", sh.name.trim(), pct, masked_email(e)),
                            None => format!("{} {}", sh.name.trim(), pct),
                        }
                    })
                    .collect();
                let value = if people.is_empty() {
                    "공동 권리자 없음".to_string()
                } else {
                    people.join(", ")
                };
                lines.push(line(track, value));
            }
            (
                "SPLITS",
                Some(release),
                payload,
                summary(
                    "수익 분배 변경",
                    lines,
                    "새로 추가한 사람에게는 수락 메일이 가요. 비율이 바뀐 사람은 다시 수락해야 하고, 목록에서 빠진 사람의 분배는 끝나요.",
                ),
            )
        }
        "request_release_end" => {
            let release = uuid_arg(args, "release_id")?;
            let title = release_title(s, a, p.org, release).await?;
            let mut payload = json!({
                "end_date": str_arg(args, "end_date"),
                "reason": str_arg(args, "reason"),
                "note": str_arg(args, "note"),
            });
            if !str_arg(args, "transfer_to").is_empty() {
                payload["transfer_to"] = json!(str_arg(args, "transfer_to"));
            }
            let _: crate::contract_terms::TermInput = parse(payload.clone())?;
            let reason = match str_arg(args, "reason") {
                "CONTRACT_END" => "계약 종료",
                "LICENSE_EXPIRY" => "이용 허락 기간 만료",
                "TRANSFER" => "다른 유통사로 이전",
                other => other,
            };
            let mut lines = vec![
                line("발매", title),
                line("종료일", str_arg(args, "end_date")),
                line("사유", reason),
            ];
            if !str_arg(args, "transfer_to").is_empty() {
                lines.push(line("이전할 유통사", str_arg(args, "transfer_to")));
            }
            (
                "RELEASE_END",
                Some(release),
                payload,
                summary(
                    "발매 서비스 종료 예약",
                    lines,
                    "종료일에 모든 플랫폼에서 이 발매를 내려요. 종료일 전에는 예약을 취소할 수 있어요.",
                ),
            )
        }
        "request_contract_end" => {
            let payload =
                json!({"end_date": str_arg(args, "end_date"), "note": str_arg(args, "note")});
            let _: crate::contract_terms::OrgTermInput = parse(payload.clone())?;
            (
                "CONTRACT_END",
                None,
                payload,
                summary(
                    "AUDENIQ 계약 종료 예약",
                    vec![line("종료일", str_arg(args, "end_date"))],
                    "종료일이 정해지면 새 발매를 신청할 수 없고, 종료일에 이 작업 공간의 모든 발매 서비스가 끝나요. 종료일 전에는 예약을 취소할 수 있어요.",
                ),
            )
        }
        "request_counter_notice" => {
            let claim = uuid_arg(args, "claim_id")?;
            let (release, work): (Option<Uuid>, String) = sqlx::query_as(
                "SELECT release_id, work FROM portal.infringement_claims WHERE id=$1 AND org_id=$2",
            )
            .bind(claim)
            .bind(p.org)
            .fetch_optional(&s.pool)
            .await
            .map_err(Error::from)?
            .ok_or(Error::NotFound)?;
            let release = release.ok_or(Error::NotFound)?;
            let title = release_title(s, a, p.org, release).await?;
            let payload = json!({"claim_id": claim, "statement": str_arg(args, "statement")});
            let _: crate::infringement::CounterInput =
                parse(json!({"statement": str_arg(args, "statement")}))?;
            (
                "COUNTER_NOTICE",
                Some(release),
                payload,
                summary(
                    "침해 신고 이의 제기",
                    vec![
                        line("발매", title),
                        line("신고된 저작물", work),
                        line("이의 제기 내용", str_arg(args, "statement")),
                    ],
                    "이의 제기는 법적 효력이 있는 진술이에요. 사실과 다르면 책임을 질 수 있으니 내용을 꼭 확인해 주세요.",
                ),
            )
        }
        "request_member_invite" => {
            let payload = json!({"email": str_arg(args, "email"), "role": str_arg(args, "role")});
            let _: crate::members::InviteInput = parse(payload.clone())?;
            let role = match str_arg(args, "role") {
                "EDITOR" => "편집자 (발매 작성·신청)",
                "VIEWER" => "보기 전용",
                other => other,
            };
            (
                "MEMBER_INVITE",
                None,
                payload,
                summary(
                    "멤버 초대",
                    vec![
                        line("초대할 계정", masked_email(str_arg(args, "email"))),
                        line("역할", role),
                    ],
                    "초대받은 사람이 수락하면 이 작업 공간의 발매와 정보를 역할만큼 볼 수 있어요.",
                ),
            )
        }
        "request_large_payout" => {
            let amount = args["amount"].as_i64().unwrap_or(0);
            let amount = amount.to_string();
            let q = crate::portal::quote_payout(s, a, p.org, &amount).await?;
            if q["within_balance"] != true {
                return Err(Error::PolicyGate("PAYOUT_EXCEEDS_BALANCE").into());
            }
            let account = crate::portal::get_payout_account(s, a, p.org).await?;
            if account["registered"] != true {
                return Err(Error::PolicyGate("PAYOUT_ACCOUNT_REQUIRED").into());
            }
            let bank = format!(
                "{} {}",
                account["bank_name"].as_str().unwrap_or(""),
                account["account_last4"]
                    .as_str()
                    .map(|l| format!("****{l}"))
                    .unwrap_or_default()
            );
            (
                "PAYOUT",
                None,
                json!({"amount": amount}),
                summary(
                    "지급 요청",
                    vec![
                        line("요청 금액", won(&q["amount"])),
                        line("수수료", won(&q["fee"])),
                        line("실수령액", won(&q["net"])),
                        line("받을 계좌", bank.trim().to_string()),
                    ],
                    "승인하면 바로 지급 요청이 접수돼요. 은행으로 보내기 전까지는 정산 화면에서 취소할 수 있어요.",
                ),
            )
        }
        _ => return Err(tool_error("UNKNOWN_TOOL", "Unknown tool.")),
    })
}

fn action_label(action: &str) -> &'static str {
    match action {
        "WITHDRAW" => "발매 신청 취소",
        "TAKEDOWN" => "서비스 중단",
        "SPLITS" => "수익 분배 변경",
        "RELEASE_END" => "발매 서비스 종료 예약",
        "CONTRACT_END" => "계약 종료 예약",
        "COUNTER_NOTICE" => "침해 신고 이의 제기",
        "MEMBER_INVITE" => "멤버 초대",
        "PAYOUT" => "지급 요청",
        _ => "작업",
    }
}

pub fn approve_url(s: &AppState, id: Uuid) -> String {
    format!("{}/ai/actions/{id}", super::issuer(s))
}

fn request_view(s: &AppState, r: &sqlx::postgres::PgRow) -> Value {
    let id: Uuid = r.get("id");
    let mut v = json!({
        "view": "action_request",
        "request_id": id,
        "action": r.get::<String, _>("action"),
        "status": r.get::<String, _>("status"),
        "summary": r.get::<Value, _>("summary"),
        "approve_url": approve_url(s, id),
        "expires_at": r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at"),
    });
    if let Some(code) = r.get::<Option<String>, _>("error_code") {
        v["error_code"] = json!(code);
    }
    if let Some(result) = r.get::<Option<Value>, _>("result") {
        v["result"] = result;
    }
    v
}

const VIEW_COLUMNS: &str = "id,action,status,summary,expires_at,error_code,result";

/// Stores the request (or returns the same open one) and tells the artist.
async fn file(
    s: &AppState,
    p: &Principal,
    action: &'static str,
    release: Option<Uuid>,
    payload: Value,
    summary: Value,
) -> ToolResult {
    expire(s, Owner::Grant(p.grant)).await?;
    if let Some(r) = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {VIEW_COLUMNS} FROM identity.mcp_action_requests
          WHERE grant_id=$1 AND action=$2 AND release_id IS NOT DISTINCT FROM $3 AND payload=$4
            AND status='PENDING' AND expires_at > now()"
    )))
    .bind(p.grant)
    .bind(action)
    .bind(release)
    .bind(&payload)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?
    {
        return Ok(with_instruction(request_view(s, &r)));
    }
    auth::rate(
        &s.pool,
        &format!("mcp-action:{}", p.grant),
        REQUESTS_PER_WINDOW,
    )
    .await?;
    let id = Uuid::new_v4();
    let r = sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO identity.mcp_action_requests(id,grant_id,user_id,org_id,client_name,action,release_id,payload,summary,expires_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,now()+make_interval(hours => $10))
         RETURNING {VIEW_COLUMNS}"
    )))
    .bind(id)
    .bind(p.grant)
    .bind(p.actor.user)
    .bind(p.org)
    .bind(&p.client_name)
    .bind(action)
    .bind(release)
    .bind(&payload)
    .bind(&summary)
    .bind(REQUEST_HOURS)
    .fetch_one(&s.pool)
    .await
    .map_err(Error::from)?;
    super::safety::notice(
        &s.pool,
        p.org,
        p.actor.user,
        &format!("AI 앱이 승인을 요청했어요: {}", action_label(action)),
        &format!(
            "{}에서 {} 요청했어요. {}시간 안에 내용을 확인하고 승인하거나 거절해 주세요. 내가 요청한 게 아니라면 거절하고 연결을 끊어 주세요.",
            p.client_name,
            super::voice::josa(action_label(action), "을", "를"),
            REQUEST_HOURS
        ),
        &format!("/ai/actions/{id}"),
    )
    .await;
    Ok(with_instruction(request_view(s, &r)))
}

fn with_instruction(mut v: Value) -> Value {
    v["instruction"] = json!(
        "Nothing has happened yet. Show approve_url to the artist and ask them to check the details in AUDENIQ Studio and approve. Call get_approval_status after they say they did."
    );
    v
}

/// `get_action_request`.
async fn status(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let id = uuid_arg(args, "request_id")?;
    expire(s, Owner::Grant(p.grant)).await?;
    let r = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {VIEW_COLUMNS} FROM identity.mcp_action_requests WHERE id=$1 AND grant_id=$2"
    )))
    .bind(id)
    .bind(p.grant)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?
    .ok_or_else(|| tool_error("NOT_FOUND", "No such request on this connection."))?;
    Ok(request_view(s, &r))
}

/// `withdraw_action_request`: this connection takes back a request the
/// artist has not answered yet. Approval claims a request (RUNNING) under a
/// row lock, so a withdrawal and an approval never both win.
async fn withdraw(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let id = uuid_arg(args, "request_id")?;
    expire(s, Owner::Grant(p.grant)).await?;
    let done = sqlx::query(
        "UPDATE identity.mcp_action_requests SET status='WITHDRAWN', decided_at=now()
          WHERE id=$1 AND grant_id=$2 AND status='PENDING'",
    )
    .bind(id)
    .bind(p.grant)
    .execute(&s.pool)
    .await
    .map_err(Error::from)?
    .rows_affected();
    let v = status(s, p, args).await?;
    if done == 0 {
        return Err(not_pending(&v["status"]));
    }
    Ok(v)
}

/// Whose requests [`expire`] looks at.
#[derive(Clone, Copy)]
pub(super) enum Owner {
    Grant(Uuid),
    User(Uuid),
}

/// The error for a request that is no longer waiting for the artist.
pub(super) fn not_pending(status: &Value) -> ToolError {
    let mut e = tool_error(
        "AI_REQUEST_DECIDED",
        "This request is no longer waiting for the artist (already approved, rejected, expired or withdrawn), so it cannot be withdrawn. If it was carried out, undo it with cancel_request or ask the artist to do it in AUDENIQ Studio.",
    );
    e.details = Some(json!({"status": status}));
    e
}

/// `withdraw_pending_submission`: this connection's application for the
/// release that still waits for the artist's approval in the Studio. The
/// Studio's approval claims it first (`safety::take`), so it is either
/// submitted or withdrawn, never both.
async fn withdraw_submission(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE identity.mcp_pending_submissions SET status='WITHDRAWN', decided_at=now()
          WHERE org_id=$1 AND release_id=$2 AND grant_id=$3 AND status='PENDING' AND decided_at IS NULL
          RETURNING id",
    )
    .bind(p.org)
    .bind(release)
    .bind(p.grant)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?;
    let Some(id) = id else {
        return Err(tool_error(
            "NOT_FOUND",
            "No application from this connection is waiting for the artist's approval for this release. A submitted application is withdrawn with request_approval (action withdrawal).",
        ));
    };
    Ok(
        json!({"view": "pending_approval", "release_id": release, "approval_id": id, "approval_status": "WITHDRAWN",
        "next_steps": ["The application was withdrawn before the artist approved it; nothing was submitted."]}),
    )
}

/// Marks the owner's (one connection's, or one user's) overdue requests
/// EXPIRED: only their own rows, so a status read stays an index lookup.
async fn expire(s: &AppState, owner: Owner) -> Result<()> {
    let (sql, id) = match owner {
        Owner::Grant(id) => (
            "UPDATE identity.mcp_action_requests SET status='EXPIRED', decided_at=now() WHERE grant_id=$1 AND status='PENDING' AND expires_at < now()",
            id,
        ),
        Owner::User(id) => (
            "UPDATE identity.mcp_action_requests SET status='EXPIRED', decided_at=now() WHERE user_id=$1 AND status='PENDING' AND expires_at < now()",
            id,
        ),
    };
    sqlx::query(sql).bind(id).execute(&s.pool).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Special-content options (artist confirmation in the conversation)
// ---------------------------------------------------------------------------

/// `set_special_options`: the draft's cover, sample, AI and express-release
/// details, written where the Studio editor keeps them (`draft.options`).
async fn special_options(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    confirmed(args)?;
    let release = uuid_arg(args, "release_id")?;
    let (r, _) = super::tools::load(s, &p.actor, p.org, release).await?;
    if !crate::submission::is_editable_status(r["status"].as_str().unwrap_or("")) {
        return Err(tool_error(
            "RELEASE_NOT_EDITABLE",
            "This release is under review and cannot be edited.",
        ));
    }
    let tracks: Vec<String> = r["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["id"].as_str().map(str::to_owned))
        .collect();
    let mut draft = r["draft"].clone();
    if !draft["options"].is_object() {
        draft["options"] = json!({});
    }
    let o = &mut draft["options"];
    let text = |v: &Value, max: usize| -> String {
        v.as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(max)
            .collect()
    };
    let strings = |v: &Value, max: usize| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .map(|x| text(x, max))
            .filter(|x| !x.is_empty())
            .take(10)
            .collect()
    };
    if let Some(c) = args.get("cover") {
        let on = c["is_cover"] == true;
        o["cover"] = json!(on);
        if on {
            let mut list = Vec::new();
            for t in c["tracks"].as_array().into_iter().flatten() {
                let id = t["track_id"].as_str().unwrap_or("");
                if !tracks.iter().any(|x| x == id) {
                    return Err(tool_error(
                        "INVALID_ARGUMENTS",
                        "cover.tracks has a track_id that is not on this release.",
                    ));
                }
                list.push(json!({
                    "trackId": id,
                    "originalTitle": text(&t["original_title"], 200),
                    "originalArtist": text(&t["original_artist"], 200),
                    "originalWriters": text(&t["original_writers"], 300),
                }));
            }
            o["coverTracks"] = json!(list);
            o["coverRightsAck"] = json!(c["rights_confirmed"] == true);
            if let Some(asset) = c["license_asset_id"].as_str() {
                o["coverLicenseAssetId"] = json!(asset);
                o["coverLicenseFile"] = json!(text(&c["license_file_name"], 200));
            }
        }
    }
    if let Some(sm) = args.get("sample") {
        let on = sm["contains_samples"] == true;
        o["sample"] = json!(on);
        if on && let Some(asset) = sm["license_asset_id"].as_str() {
            o["sampleLicenseAssetId"] = json!(asset);
            o["sampleLicenseFile"] = json!(text(&sm["license_file_name"], 200));
        }
    }
    if let Some(ai) = args.get("ai") {
        let on = ai["involved"] == true;
        o["ai"] = json!(on);
        if on {
            let uses = strings(&ai["uses"], 200);
            let tools = strings(&ai["tools"], 100);
            if uses.is_empty() {
                return Err(tool_error(
                    "INVALID_ARGUMENTS",
                    "ai.uses must say what AI was used for.",
                ));
            }
            let mut summary = uses.join(" · ");
            if !tools.is_empty() {
                summary = format!("{summary} / 도구: {}", tools.join(", "));
            }
            o["aiUses"] = json!(uses);
            o["aiTools"] = json!(tools);
            o["aiUseOther"] = json!("");
            o["aiToolOther"] = json!("");
            o["aiTool"] = json!(summary);
        }
    }
    if let Some(e) = args.get("express") {
        let on = e["requested"] == true;
        o["express"] = json!(on);
        if on {
            o["expressReason"] = json!(text(&e["reason"], 300));
            o["expressAck"] = json!(e["terms_accepted"] == true);
        }
    }
    if let Some(m) = args.get("minor") {
        let on = m["is_minor"] == true;
        o["minor"] = json!(on);
        if on {
            o["guardian"] = json!(text(&m["guardian_name"], 90));
            o["guardianRelation"] = json!(text(&m["guardian_relation"], 20));
            o["guardianContact"] = json!(text(&m["guardian_contact"], 160));
        }
    }
    let options = draft["options"].clone();
    super::tools::write_release(s, &p.actor, p.org, release, &r, draft, &json!({})).await?;
    Ok(json!({
        "view": "special_options",
        "release_id": release,
        "options": options,
        "note": "Declarations at submission must match these options (is_cover, contains_samples, ai_involved). Run validate_release next.",
    }))
}

// ---------------------------------------------------------------------------
// Studio (session + CSRF)
// ---------------------------------------------------------------------------

const REQUEST_JSON: &str = "jsonb_build_object('id',q.id,'action',q.action,'status',q.status,'summary',q.summary,
    'client_name',q.client_name,'org_id',q.org_id,'org_name',o.name,'release_id',q.release_id,'created_at',q.created_at,
    'expires_at',q.expires_at,'decided_at',q.decided_at,'error_code',q.error_code,'result',q.result,
    'connection_label',g.label,'connection_revoked',g.revoked_at IS NOT NULL)";

/// The user's open requests (Studio: Profile → Connected AI apps).
pub async fn list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    expire(&s, Owner::User(a.user)).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_JSON} FROM identity.mcp_action_requests q
           JOIN identity.mcp_grants g ON g.id=q.grant_id JOIN identity.orgs o ON o.id=q.org_id
          WHERE q.user_id=$1 AND q.status='PENDING' AND g.revoked_at IS NULL ORDER BY q.created_at DESC LIMIT 50"
    )))
    .bind(a.user)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!({"items": items})))
}

/// One request (the approval page), only the signed-in user's own.
pub async fn get(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    expire(&s, Owner::User(a.user)).await?;
    let v: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_JSON} FROM identity.mcp_action_requests q
           JOIN identity.mcp_grants g ON g.id=q.grant_id JOIN identity.orgs o ON o.id=q.org_id
          WHERE q.id=$1 AND q.user_id=$2"
    )))
    .bind(id)
    .bind(a.user)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(Json(v))
}

pub async fn approve(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Box::pin(decide(&s, &h, id, true)).await
}

pub async fn reject(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Box::pin(decide(&s, &h, id, false)).await
}

async fn decide(s: &AppState, h: &HeaderMap, id: Uuid, approve: bool) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    // The request and its connection, locked: a revoke, another tab or the
    // expiry cannot race the decision. Approval claims it (RUNNING) before
    // the action runs, so it runs at most once.
    let row = sqlx::query(
        "SELECT q.grant_id, q.org_id, q.action, q.release_id, q.payload, q.status,
                q.expires_at < now() AS expired, g.revoked_at IS NOT NULL AS revoked
           FROM identity.mcp_action_requests q JOIN identity.mcp_grants g ON g.id=q.grant_id
          WHERE q.id=$1 AND q.user_id=$2 FOR UPDATE OF q, g",
    )
    .bind(id)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    match row.get::<String, _>("status").as_str() {
        "PENDING" => {}
        "WITHDRAWN" => return Err(Error::InvalidCode("AI_REQUEST_WITHDRAWN")),
        _ => return Err(Error::InvalidCode("AI_REQUEST_DECIDED")),
    }
    let close = |status: &'static str| {
        sqlx::query(
            "UPDATE identity.mcp_action_requests SET status=$2, decided_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(status)
    };
    if row.get::<bool, _>("expired") {
        close("EXPIRED").execute(&mut *tx).await?;
        tx.commit().await?;
        return Err(Error::InvalidCode("AI_REQUEST_EXPIRED"));
    }
    if row.get::<bool, _>("revoked") {
        close("REVOKED").execute(&mut *tx).await?;
        tx.commit().await?;
        return Err(Error::InvalidCode("AI_CONNECTION_REVOKED"));
    }
    let org: Uuid = row.get("org_id");
    let action: String = row.get("action");
    let release: Option<Uuid> = row.get("release_id");
    let payload: Value = row.get("payload");
    close(if approve { "RUNNING" } else { "REJECTED" })
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        if approve {
            "mcp.action_approved"
        } else {
            "mcp.action_rejected"
        },
        &action,
        a.request,
    )
    .await?;
    tx.commit().await?;
    if !approve {
        return Ok(Json(json!({"id": id, "status": "REJECTED"})));
    }
    let outcome = Box::pin(run(s, &a, org, id, &action, release, payload)).await;
    let (status, result, code) = match &outcome {
        Ok(v) => ("DONE", Some(v.clone()), None),
        Err(e) => ("FAILED", None, Some(super::protocol::error_code(e))),
    };
    sqlx::query(
        "UPDATE identity.mcp_action_requests SET status=$2, result=$3, error_code=$4 WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(&result)
    .bind(code)
    .execute(&s.pool)
    .await?;
    // The artist sees why it failed (same message as doing it in the Studio).
    outcome?;
    Ok(Json(json!({"id": id, "status": status, "result": result})))
}

fn input<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| Error::Invalid)
}

/// Runs an approved action with the approving artist's session.
async fn run(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    action: &str,
    release: Option<Uuid>,
    payload: Value,
) -> Result<Value> {
    let release = || release.ok_or(Error::Invalid);
    match action {
        "WITHDRAW" => crate::withdraw::withdraw_by_artist(&s.pool, a, org, release()?).await,
        "TAKEDOWN" => crate::takedown::request(s, a, org, release()?, input(payload)?).await,
        "SPLITS" => crate::splits::save(s, a, org, release()?, input(payload)?).await,
        "RELEASE_END" => {
            crate::contract_terms::set_release_term(s, a, org, release()?, input(payload)?).await
        }
        "CONTRACT_END" => crate::contract_terms::set_org_term(s, a, org, input(payload)?).await,
        "COUNTER_NOTICE" => {
            let claim: Uuid = payload["claim_id"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .ok_or(Error::Invalid)?;
            let statement = json!({"statement": payload["statement"]});
            crate::infringement::counter(s, a, org, claim, input(statement)?).await
        }
        "MEMBER_INVITE" => crate::members::invite(s, a, org, input(payload)?).await,
        "PAYOUT" => {
            let i = crate::portal::PayoutRequestInput {
                amount: payload["amount"].as_str().unwrap_or("").to_string(),
                idempotency_key: format!("mcp-approval:{id}"),
            };
            crate::portal::request_payout(s, a, org, i).await
        }
        _ => Err(Error::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_format_won_and_mask_addresses() {
        assert_eq!(won(&json!("1234567")), "1,234,567원");
        assert_eq!(won(&json!("500.00")), "500원");
        assert_eq!(masked_email("hana@example.com"), "h***@example.com");
    }
}
