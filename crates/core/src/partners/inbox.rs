//! Partner webhook inbox (worker side) and staff live evidence.
//!
//! The API verifies a webhook's signature and files the raw body in
//! `execution.partner_inbox` (it holds no execution write grants). The
//! `delivery.ack` job parses it with the partner's adapter, resolves the
//! org from the ids it carries and applies it through the same
//! deduplicated path as polled ACK files.
use crate::error::{Error, Result};
use crate::execution::AdapterRegistry;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

async fn finish(pool: &PgPool, id: Uuid, result: &str) -> Result<()> {
    sqlx::query(
        "UPDATE execution.partner_inbox SET processed_at=now(), result=$2 WHERE id=$1 AND processed_at IS NULL",
    )
    .bind(id)
    .bind(result.chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(())
}

/// Apply one filed webhook. Returns the recorded result.
pub async fn process(pool: &PgPool, registry: &AdapterRegistry, id: Uuid) -> Result<String> {
    let row: Option<(String, Vec<u8>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT partner_id, payload, processed_at FROM execution.partner_inbox WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((partner_id, payload, processed)) = row else {
        return Err(Error::NotFound);
    };
    if processed.is_some() {
        return Ok("ALREADY_PROCESSED".into());
    }
    let Some(adapter) = registry.get(&partner_id) else {
        finish(pool, id, "NO_ADAPTER").await?;
        return Ok("NO_ADAPTER".into());
    };
    if !adapter.capabilities().parse_ack {
        finish(pool, id, "PARSE_ACK_DISABLED").await?;
        return Ok("PARSE_ACK_DISABLED".into());
    }
    let event = match adapter.parse_ack(&payload).await {
        Ok(e) => e,
        Err(Error::InvalidCode("ACK_PENDING")) => {
            finish(pool, id, "PENDING_NOTICE").await?;
            return Ok("PENDING_NOTICE".into());
        }
        Err(e) => {
            let r = format!("UNPARSEABLE:{e}");
            finish(pool, id, &r).await?;
            return Ok(r);
        }
    };
    let Some(org) = crate::execution::org_for_event(pool, &partner_id, &event).await? else {
        finish(pool, id, "UNMATCHED").await?;
        return Ok("UNMATCHED".into());
    };
    let r = crate::execution::apply_ack_event(pool, org, &partner_id, &event).await?;
    finish(pool, id, &r).await?;
    tracing::info!(partner_id, inbox_id=%id, result=%r, "partner webhook applied");
    Ok(r)
}

/// `delivery.mark_live` payload: package_id, partner_id, optional
/// partner_release_id and staff_user_id.
pub async fn mark_live(pool: &PgPool, payload: &Value) -> Result<String> {
    let uuid = |k: &str| {
        payload
            .get(k)
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
    };
    let package = uuid("package_id").ok_or(Error::Invalid)?;
    let partner = payload
        .get("partner_id")
        .and_then(Value::as_str)
        .filter(|p| crate::partner_config::valid_partner_id(p))
        .ok_or(Error::Invalid)?;
    let prid = payload
        .get("partner_release_id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    crate::execution::record_manual_live(pool, package, partner, prid, uuid("staff_user_id")).await
}
