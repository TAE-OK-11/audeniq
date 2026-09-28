//! `POST /api/partner-hooks/{partner_id}`: inbound partner notifications
//! (ingestion ACKs, live and takedown confirmations).
//!
//! The request reaches the API through the edge Worker like every other
//! call (service secret), but carries no browser Origin: its authenticity
//! is the partner's HMAC signature, checked against the webhook secret in
//! the partner's config file (`webhook` section). A verified body is filed
//! in `execution.partner_inbox` and a `delivery.ack` job applies it on the
//! worker; the API never writes delivery state itself.
use crate::api::AppState;
use crate::error::{Error, Result};
use crate::partner_config::{PartnerConfig, valid_partner_id};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const HOOK_PREFIX: &str = "/api/partner-hooks/";

type ConfigCache = Mutex<HashMap<String, (Instant, Option<PartnerConfig>)>>;

fn config_cache() -> &'static ConfigCache {
    static C: OnceLock<ConfigCache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn partner_config(partner_id: &str) -> Option<PartnerConfig> {
    if let Ok(c) = config_cache().lock()
        && let Some((at, cfg)) = c.get(partner_id)
        && at.elapsed() < Duration::from_secs(60)
    {
        return cfg.clone();
    }
    let cfg = crate::partner_config::load(partner_id).ok().flatten();
    if let Ok(mut c) = config_cache().lock() {
        c.insert(partner_id.to_string(), (Instant::now(), cfg.clone()));
    }
    cfg
}

async fn receive(
    State(s): State<AppState>,
    Path(partner_id): Path<String>,
    h: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    if !valid_partner_id(&partner_id) || body.is_empty() {
        return Err(Error::NotFound);
    }
    let cfg = partner_config(&partner_id).ok_or(Error::NotFound)?;
    let hook = cfg.webhook.as_ref().ok_or(Error::NotFound)?;
    let secret = hook.secret.resolve().map_err(|_| Error::NotFound)?;
    let header = |name: &str| h.get(name).and_then(|v| v.to_str().ok());
    let signature = header(&hook.signature_header).ok_or(Error::Unauthorized)?;
    let timestamp = match &hook.timestamp_header {
        Some(t) => Some(header(t).ok_or(Error::Unauthorized)?),
        None => None,
    };
    if !crate::partners::http::verify_webhook(
        secret.expose(),
        &body,
        signature,
        timestamp,
        hook.max_skew_secs.clamp(30, 3600),
        chrono::Utc::now().timestamp(),
    ) {
        tracing::warn!(partner_id, "partner webhook signature rejected");
        return Err(Error::Unauthorized);
    }
    let sha = crate::partners::sha256_hex(&body);
    let mut tx = s.pool.begin().await?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO execution.partner_inbox(id,partner_id,payload,payload_sha256,content_type)
         VALUES($1,$2,$3,$4,$5) ON CONFLICT (partner_id,payload_sha256) DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(&partner_id)
    .bind(body.as_ref())
    .bind(&sha)
    .bind(header("content-type").map(|c| c.chars().take(100).collect::<String>()))
    .fetch_optional(&mut *tx)
    .await?;
    let Some(id) = id else {
        // The same body again (partner retry): already filed.
        tx.rollback().await?;
        return Ok((
            StatusCode::OK,
            Json(json!({"received": true, "duplicate": true})),
        ));
    };
    crate::operations::enqueue(
        &mut tx,
        "delivery",
        "delivery.ack",
        &json!({"inbox_id": id}),
        &format!("delivery.ack:{id}"),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"received": true}))))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/partner-hooks/{partner_id}", post(receive))
}
