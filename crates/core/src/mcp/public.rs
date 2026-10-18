//! Public tools: AUDENIQ information that needs no account (notices,
//! events, service status, release requirements).
//!
//! Without a token a client may call them [`Settings::public_daily_anonymous`]
//! times per IP address per day (KST); after that it gets 401 and the
//! assistant asks the artist to connect an account. A connected user gets
//! [`Settings::public_daily_user`] calls a day (or the limit an operator set
//! for them in `identity.mcp_usage_limits`) and is then pointed to support.
//!
//! [`Settings::public_daily_anonymous`]: super::Settings::public_daily_anonymous
//! [`Settings::public_daily_user`]: super::Settings::public_daily_user
use super::oauth::Principal;
use super::tools::{
    ToolDef, ToolError, err_result, invalid_arguments, ok_result, requirements, tool_error,
};
use crate::{
    api::AppState,
    auth,
    error::Result,
    site_content::{self, Table},
};
use chrono::{Duration, FixedOffset, TimeZone, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub enum Caller<'a> {
    /// No token: counted per client IP (the edge's `x-audeniq-client-ip`).
    Anonymous {
        source: String,
    },
    User(&'a Principal),
}

pub enum Outcome {
    /// The JSON-RPC `result` of `tools/call`.
    Result(Value),
    /// The anonymous daily allowance is used up: answer 401 so the client
    /// starts account linking.
    SignInRequired { limit: i32, resets_at: String },
}

/// The next midnight in Korea, when the daily counts start over.
fn resets_at() -> String {
    let kst = FixedOffset::east_opt(9 * 3600).expect("KST offset");
    let tomorrow = Utc::now().with_timezone(&kst).date_naive() + Duration::days(1);
    kst.from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("KST has no gaps")
        .with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Counts one call for `key` today (KST) and returns today's total.
async fn count(pool: &PgPool, key: &str) -> Result<i32> {
    let calls: i32 = sqlx::query_scalar(
        "INSERT INTO identity.mcp_daily_usage(bucket_hash, day, calls) VALUES($1, (now() AT TIME ZONE 'Asia/Seoul')::date, 1)
         ON CONFLICT (bucket_hash, day) DO UPDATE SET calls = identity.mcp_daily_usage.calls + 1
         RETURNING calls",
    )
    .bind(auth::bucket_hash(key))
    .fetch_one(pool)
    .await?;
    if calls == 1 {
        // A new bucket for today: drop counts older than yesterday.
        sqlx::query("DELETE FROM identity.mcp_daily_usage WHERE day < (now() AT TIME ZONE 'Asia/Seoul')::date - 1")
            .execute(pool)
            .await?;
    }
    Ok(calls)
}

async fn user_limit(s: &AppState, user: Uuid) -> Result<i32> {
    let own: Option<i32> =
        sqlx::query_scalar("SELECT daily_limit FROM identity.mcp_usage_limits WHERE user_id=$1")
            .bind(user)
            .fetch_optional(&s.pool)
            .await?;
    Ok(own.unwrap_or(s.mcp.public_daily_user))
}

pub async fn call(s: &AppState, def: &ToolDef, args: Value, caller: Caller<'_>) -> Result<Outcome> {
    let (key, limit, signed_in) = match &caller {
        Caller::Anonymous { source } => (
            format!("mcp-public:ip:{source}"),
            s.mcp.public_daily_anonymous,
            false,
        ),
        Caller::User(p) => (
            format!("mcp-public:user:{}", p.actor.user),
            user_limit(s, p.actor.user).await?,
            true,
        ),
    };
    let used = count(&s.pool, &key).await?;
    if used > limit {
        tracing::info!(tool = def.name, signed_in, limit, "mcp_public_limit");
        if !signed_in {
            return Ok(Outcome::SignInRequired {
                limit,
                resets_at: resets_at(),
            });
        }
        let mut e = tool_error(
            "DAILY_LIMIT_REACHED",
            "Today's limit for AUDENIQ's public information is used up. To use more, the artist asks AUDENIQ support to raise it (create_inquiry, or the support page in AUDENIQ Studio).",
        );
        e.details = Some(json!({
            "limit": limit,
            "resets_at": resets_at(),
            "support_url": format!("{}/inquiries", super::issuer(s)),
        }));
        return Ok(Outcome::Result(err_result(e)));
    }
    if let Some(invalid) = invalid_arguments(def, &args) {
        return Ok(Outcome::Result(invalid));
    }
    let outcome = run(s, def, &args).await;
    tracing::info!(tool = def.name, signed_in, ok = outcome.is_ok(), "mcp_tool");
    Ok(Outcome::Result(match outcome {
        Ok(mut v) => {
            v["usage"] = json!({
                "signed_in": signed_in,
                "limit": limit,
                "used_today": used,
                "remaining_today": (limit - used).max(0),
            });
            ok_result(v.to_string(), v)
        }
        Err(e) => err_result(e),
    }))
}

async fn run(s: &AppState, def: &ToolDef, args: &Value) -> std::result::Result<Value, ToolError> {
    match def.name {
        "get_release_requirements" => Ok(requirements()),
        "list_announcements" => announcements(s, args).await,
        "get_announcement" => announcement(s, args).await,
        "get_service_status" => Ok(json!({
            "view": "service_status",
            "status": site_content::service_status(s, site_content::now_utc()).await,
        })),
        _ => Err(tool_error("UNKNOWN_TOOL", "Not a public tool.")),
    }
}

fn table(args: &Value) -> Table {
    match args["kind"].as_str() {
        Some("events") => Table::Events,
        _ => Table::Notices,
    }
}

/// A Studio link for one notice, or the events page.
fn link(s: &AppState, table: Table, id: &str) -> String {
    match table {
        Table::Notices => format!("{}/notices/{id}", super::issuer(s)),
        _ => format!("{}/events", super::issuer(s)),
    }
}

fn excerpt(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 200 {
        flat
    } else {
        let mut out: String = flat.chars().take(200).collect();
        out.push('…');
        out
    }
}

async fn announcements(s: &AppState, args: &Value) -> std::result::Result<Value, ToolError> {
    let table = table(args);
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let rows = site_content::published(s, table).await?;
    let total = rows.len();
    let items: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .map(|mut r| {
            let id = r["id"].as_str().unwrap_or_default().to_string();
            let body = r["body"].as_str().map(excerpt).unwrap_or_default();
            if let Some(o) = r.as_object_mut() {
                o.remove("body");
                o.insert("excerpt".into(), json!(body));
                o.insert("url".into(), json!(link(s, table, &id)));
            }
            r
        })
        .collect();
    Ok(json!({
        "view": "announcements",
        "kind": if table == Table::Events { "events" } else { "notices" },
        "total": total,
        "items": items,
    }))
}

async fn announcement(s: &AppState, args: &Value) -> std::result::Result<Value, ToolError> {
    let table = table(args);
    let id = args["id"].as_str().unwrap_or_default();
    let Some(mut row) = site_content::published_one(s, table, id).await? else {
        return Err(tool_error(
            "NOT_FOUND",
            "No published notice or event has this id. Call list_announcements for current ones.",
        ));
    };
    row["url"] = json!(link(s, table, id));
    row["view"] = json!("announcement");
    Ok(row)
}

// ---------------------------------------------------------------------------
// Operator commands (audeniq-admin mcp-limit)
// ---------------------------------------------------------------------------

/// Sets one user's daily limit for the public tools (after a support
/// inquiry). `None` returns them to the default.
pub async fn admin_set_limit(
    pool: &PgPool,
    operator: &str,
    user: Uuid,
    limit: Option<i32>,
    note: Option<&str>,
) -> anyhow::Result<()> {
    let operator = operator.trim();
    anyhow::ensure!(!operator.is_empty(), "--operator NAME is required");
    let mut tx = pool.begin().await?;
    match limit {
        Some(n) => {
            anyhow::ensure!((0..=100_000).contains(&n), "limit must be 0-100000");
            sqlx::query("INSERT INTO identity.mcp_usage_limits(user_id,daily_limit,note,set_by) VALUES($1,$2,$3,$4)
                         ON CONFLICT(user_id) DO UPDATE SET daily_limit=EXCLUDED.daily_limit, note=EXCLUDED.note, set_by=EXCLUDED.set_by, updated_at=now()")
                .bind(user).bind(n).bind(note).bind(operator)
                .execute(&mut *tx).await?;
        }
        None => {
            sqlx::query("DELETE FROM identity.mcp_usage_limits WHERE user_id=$1")
                .bind(user)
                .execute(&mut *tx)
                .await?;
        }
    }
    crate::operations::audit(
        &mut tx,
        None,
        None,
        Some(user),
        "mcp.usage_limit_set",
        &format!(
            "OPERATOR:{operator} {}",
            limit.map_or("DEFAULT".to_string(), |n| n.to_string())
        ),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn admin_list_limits(pool: &PgPool) -> anyhow::Result<Value> {
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('user_id',user_id,'daily_limit',daily_limit,'note',note,'set_by',set_by,'updated_at',updated_at)
           FROM identity.mcp_usage_limits ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(json!({"items": items}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excerpt_flattens_and_cuts_long_text() {
        assert_eq!(excerpt("a\n\n b\tc"), "a b c");
        let long = "가".repeat(250);
        let e = excerpt(&long);
        assert_eq!(e.chars().count(), 201);
        assert!(e.ends_with('…'));
    }
    #[test]
    fn counts_reset_at_korean_midnight() {
        let r = resets_at();
        assert!(r.ends_with("T15:00:00Z"), "{r}");
    }
}
