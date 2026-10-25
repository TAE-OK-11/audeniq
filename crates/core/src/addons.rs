//! Add-on services (migration 0157): paid work an artist orders on top of
//! distribution, paid from the settlement balance or by card, and worked by
//! staff to the end.
//!
//! - `MV_DISTRIBUTION`: a music video to the major video DSPs. `BASIC`
//!   (already rated by 영상물등급위원회: distribution only), `REVIEW`
//!   (+ rating filed for the artist), `EXPRESS` (+ expedited handling).
//! - `EXPRESS_RELEASE`: the release is reviewed and delivered first so it can
//!   go out three business days after submission. Made by submit when the
//!   application asks for it ([`express_from_submission`]); not orderable
//!   on its own.
//! - `LYRIC_SYNC`: time-synced lyrics (LRC) made for the chosen tracks and
//!   registered with lyric services, priced per track. An artist may also
//!   upload their own LRC for free ([`put_lyric_sync`]).
//!
//! Payment: [`pay_settlement`] goes through `finance.charge_addon` (the API
//! cannot write the ledger; the function locks like payouts and refuses an
//! amount over the withdrawable balance). Card payments use Toss Payments:
//! [`card_start`] gives the browser what the payment window needs and
//! [`card_confirm`] confirms server-side, checking the amount. A cancelled
//! (artist, before work starts) or rejected (staff) paid order is refunded:
//! `finance.refund_addon` for the balance, Toss cancel for a card.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    staff::{self, Duty},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Row};
use std::{sync::OnceLock, time::Duration};
use uuid::Uuid;

pub const EXPRESS_PRICE: i32 = 10_000;
pub const LYRIC_SYNC_PRICE: i32 = 5_000;
/// Music video tiers and their prices (KRW, VAT included).
pub const MV_TIERS: &[(&str, i32)] = &[("BASIC", 19_000), ("REVIEW", 39_000), ("EXPRESS", 49_000)];
/// Video DSPs a music video can go to.
pub const MV_PLATFORMS: &[&str] = &["melon", "genie", "bugs", "flo", "vibe", "youtube"];
/// 영상물등급위원회 ratings.
const RATINGS: &[&str] = &["ALL", "12", "15", "18"];
/// Lyric services staff register synced lyrics with.
pub const LYRIC_SERVICES: &[&str] = &[
    "musixmatch",
    "lyricfind",
    "melon",
    "genie",
    "bugs",
    "flo",
    "vibe",
];
const OPEN: &[&str] = &["SUBMITTED", "IN_PROGRESS", "DELIVERED"];

fn invalid() -> Error {
    Error::InvalidCode("ADDON_INVALID")
}

fn text(v: &Value, key: &str, max: usize) -> Result<String> {
    let s = v.get(key).and_then(Value::as_str).unwrap_or("").trim();
    if s.chars().count() > max {
        return Err(invalid());
    }
    if !s.is_empty() {
        crate::text_policy::check_multiline(s)?;
    }
    Ok(s.to_string())
}

/// https only, no credentials or port (a link to the video file).
fn https_url(raw: &str) -> Result<String> {
    let bad = || Error::InvalidCode("ADDON_URL_INVALID");
    if raw.len() > 500 {
        return Err(bad());
    }
    let u = url::Url::parse(raw.trim()).map_err(|_| bad())?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || u.host_str().is_none()
    {
        return Err(bad());
    }
    Ok(u.to_string())
}

/// Count of timed lines in an LRC text; refuses anything that is not LRC
/// (`[mm:ss.xx]` lines and `[tag:value]` headers, blank lines allowed).
pub fn lrc_lines(lrc: &str) -> Result<usize> {
    let bad = || Error::InvalidCode("LRC_INVALID");
    if lrc.trim().is_empty() || lrc.chars().count() > 60_000 {
        return Err(bad());
    }
    let mut timed = 0;
    for line in lrc.lines() {
        let mut rest = line.trim();
        if rest.is_empty() {
            continue;
        }
        let mut stamps = 0;
        while let Some(body) = rest.strip_prefix('[') {
            let end = body.find(']').ok_or_else(bad)?;
            let tag = &body[..end];
            let (a, b) = tag.split_once(':').ok_or_else(bad)?;
            if a.chars().all(|c| c.is_ascii_digit()) && !a.is_empty() {
                // mm:ss(.xx)
                let (sec, frac) = b.split_once(['.', ':']).unwrap_or((b, ""));
                let ok = a.len() <= 3
                    && sec.len() == 2
                    && sec.chars().all(|c| c.is_ascii_digit())
                    && sec < "60"
                    && frac.len() <= 3
                    && frac.chars().all(|c| c.is_ascii_digit());
                if !ok {
                    return Err(bad());
                }
                stamps += 1;
            } else if !a.chars().all(|c| c.is_ascii_alphabetic()) || a.is_empty() {
                return Err(bad());
            }
            rest = &body[end + 1..];
        }
        if stamps > 0 {
            timed += 1;
        } else if !rest.is_empty() {
            // Text without a time stamp.
            return Err(bad());
        }
    }
    if timed == 0 {
        return Err(bad());
    }
    Ok(timed)
}

// ---------------------------------------------------------------------------
// Toss Payments
// ---------------------------------------------------------------------------
struct Toss {
    client_key: String,
    secret: String,
    base: String,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// Development/test only: a loopback server stands in for Toss.
fn toss_fixture() -> Option<String> {
    let base = env("TOSS_FIXTURE_BASE")?;
    let local = crate::external_http::local_http_enabled(true);
    crate::external_http::validate_url(&base, local)
        .ok()
        .map(|_| base.trim_end_matches('/').to_owned())
}

/// Card payments when both keys are set.
fn toss() -> Option<Toss> {
    Some(Toss {
        client_key: env("TOSS_CLIENT_KEY")?,
        secret: env("TOSS_SECRET_KEY")?,
        base: toss_fixture().unwrap_or_else(|| "https://api.tosspayments.com".into()),
    })
}

pub fn card_available() -> bool {
    toss().is_some()
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        crate::external_http::client_builder(toss_fixture().is_some())
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client")
    })
}

/// POST to Toss; the JSON answer, or `ADDON_CARD_FAILED` with Toss's code
/// logged (never the secret).
async fn toss_post(t: &Toss, path: &str, body: Value, idem: &str) -> Result<Value> {
    use base64::Engine;
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("{}:", t.secret));
    let r = http()
        .post(format!("{}{path}", t.base))
        .header("authorization", format!("Basic {auth}"))
        .header("content-type", "application/json")
        .header("idempotency-key", idem)
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "toss request failed");
            Error::PolicyGate("ADDON_CARD_FAILED")
        })?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        tracing::warn!(%status, code = %v["code"], "toss refused");
        return Err(Error::PolicyGate("ADDON_CARD_FAILED"));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------
const ORDER_JSON: &str = "jsonb_build_object('id',o.id,'service',o.service,'tier',o.tier,'release_id',o.release_id,
    'release_title',(SELECT r.title FROM catalog.releases r WHERE r.id=o.release_id),'track_ids',o.track_ids,
    'tracks',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',t.id,'title',t.title) ORDER BY t.disc_number,t.track_number)
                       FROM catalog.tracks t WHERE t.id = ANY(o.track_ids)),'[]'::jsonb),
    'title',o.title,'detail',o.detail,'quantity',o.quantity,'unit_price',o.unit_price,'amount',o.amount,
    'status',o.status,'pay_method',o.pay_method,'paid_at',o.paid_at,'refunded_at',o.refunded_at,
    'result',o.result,'resolution',o.resolution,'created_at',o.created_at,'updated_at',o.updated_at,
    'events',COALESCE((SELECT jsonb_agg(jsonb_build_object('status',e.status,'note',e.note,'by_staff',e.by_staff,'at',e.created_at)
                        ORDER BY e.created_at) FROM portal.addon_order_events e WHERE e.order_id=o.id),'[]'::jsonb))";

async fn event(
    c: &mut PgConnection,
    id: Uuid,
    status: &str,
    note: &str,
    by_staff: bool,
) -> Result<()> {
    sqlx::query("INSERT INTO portal.addon_order_events(order_id, status, note, by_staff) VALUES($1,$2,$3,$4)")
        .bind(id)
        .bind(status)
        .bind(note)
        .bind(by_staff)
        .execute(&mut *c)
        .await?;
    Ok(())
}

async fn order_json(c: &mut PgConnection, id: Uuid) -> Result<Value> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {ORDER_JSON} FROM portal.addon_orders o WHERE o.id=$1"
    )))
    .bind(id)
    .fetch_one(&mut *c)
    .await?)
}

pub fn prices() -> Value {
    json!({
        "mv": MV_TIERS.iter().map(|(t, p)| json!({"tier": t, "price": p})).collect::<Vec<_>>(),
        "express_release": EXPRESS_PRICE,
        "lyric_sync": LYRIC_SYNC_PRICE,
    })
}

pub async fn list(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {ORDER_JSON} FROM portal.addon_orders o WHERE o.org_id=$1 ORDER BY o.created_at DESC LIMIT 300"
    )))
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    let balance = crate::finance::withdrawable(&mut tx, org, None).await?;
    tx.rollback().await?;
    Ok(json!({
        "items": items, "prices": prices(), "card": card_available(),
        "balance": balance.trunc().to_string().parse::<i64>().unwrap_or(0).max(0),
        "mv_platforms": MV_PLATFORMS,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewOrder {
    pub service: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub release_id: Option<Uuid>,
    #[serde(default)]
    pub track_ids: Vec<Uuid>,
    #[serde(default)]
    pub detail: Value,
}

/// The order's release, checked: the caller may write it and it was submitted.
async fn submitted_release(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> Result<String> {
    auth::authorize(c, a, org, release, "release", true).await?;
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT status, NULLIF(btrim(title), '') FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    if matches!(row.0.as_str(), "DRAFT" | "SUPERSEDED") {
        return Err(Error::PolicyGate("ADDON_RELEASE_NOT_SUBMITTED"));
    }
    Ok(row.1.unwrap_or_else(|| "발매".into()))
}

/// Validated music video details for a tier.
fn mv_detail(tier: &str, d: &Value) -> Result<Value> {
    let video = https_url(d.get("video_url").and_then(Value::as_str).unwrap_or(""))?;
    let title = text(d, "mv_title", 200)?;
    if title.is_empty() {
        return Err(Error::InvalidCode("ADDON_MV_TITLE_REQUIRED"));
    }
    let duration = text(d, "duration", 8)?;
    let ok_duration = duration.split_once(':').is_some_and(|(m, s)| {
        !m.is_empty()
            && m.len() <= 2
            && m.chars().all(|c| c.is_ascii_digit())
            && s.len() == 2
            && s.chars().all(|c| c.is_ascii_digit())
            && s < "60"
    });
    if !ok_duration {
        return Err(Error::InvalidCode("ADDON_MV_DURATION_INVALID"));
    }
    let platforms: Vec<String> = d
        .get("platforms")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if platforms.is_empty()
        || platforms.len() > MV_PLATFORMS.len()
        || platforms
            .iter()
            .any(|p| !MV_PLATFORMS.contains(&p.as_str()))
    {
        return Err(Error::InvalidCode("ADDON_MV_PLATFORMS_INVALID"));
    }
    let mut out = Map::new();
    out.insert("video_url".into(), json!(video));
    out.insert("mv_title".into(), json!(title));
    out.insert("duration".into(), json!(duration));
    out.insert("platforms".into(), json!(platforms));
    for (k, max) in [
        ("director", 200),
        ("production", 200),
        ("cast", 500),
        ("synopsis", 2000),
        ("notes", 2000),
    ] {
        out.insert(k.into(), json!(text(d, k, max)?));
    }
    let date = text(d, "release_date", 10)?;
    if !date.is_empty() && chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err(invalid());
    }
    out.insert("release_date".into(), json!(date));
    if tier == "BASIC" {
        // Already rated: the rating and its number are part of the delivery.
        let rating = text(d, "rating", 3)?;
        let rating_no = text(d, "rating_no", 60)?;
        if !RATINGS.contains(&rating.as_str()) || rating_no.is_empty() {
            return Err(Error::InvalidCode("ADDON_MV_RATING_REQUIRED"));
        }
        out.insert("rating".into(), json!(rating));
        out.insert("rating_no".into(), json!(rating_no));
    }
    Ok(Value::Object(out))
}

pub async fn create(s: &AppState, a: &Actor, org: Uuid, i: NewOrder) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let (tier, unit, quantity, release, tracks, title, detail): (
        String,
        i32,
        i32,
        Uuid,
        Vec<Uuid>,
        String,
        Value,
    ) = match i.service.as_str() {
        "MV_DISTRIBUTION" => {
            let unit = MV_TIERS
                .iter()
                .find(|(t, _)| *t == i.tier)
                .map(|(_, p)| *p)
                .ok_or(Error::InvalidCode("ADDON_TIER_INVALID"))?;
            let release = i
                .release_id
                .ok_or(Error::InvalidCode("ADDON_RELEASE_REQUIRED"))?;
            submitted_release(&mut tx, a, org, release).await?;
            if i.track_ids.len() != 1 {
                return Err(Error::InvalidCode("ADDON_TRACK_REQUIRED"));
            }
            let detail = mv_detail(&i.tier, &i.detail)?;
            let title = detail["mv_title"].as_str().unwrap_or("").to_string();
            (
                i.tier.clone(),
                unit,
                1,
                release,
                i.track_ids.clone(),
                title,
                detail,
            )
        }
        "LYRIC_SYNC" => {
            let release = i
                .release_id
                .ok_or(Error::InvalidCode("ADDON_RELEASE_REQUIRED"))?;
            let rtitle = submitted_release(&mut tx, a, org, release).await?;
            let mut ids = i.track_ids.clone();
            ids.sort();
            ids.dedup();
            if ids.is_empty() || ids.len() > 100 {
                return Err(Error::InvalidCode("ADDON_TRACK_REQUIRED"));
            }
            // Every chosen track of this release, with lyrics to sync.
            let ok: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND id = ANY($3)
                        AND length(btrim(coalesce(lyrics,''))) > 0",
                )
                .bind(org)
                .bind(release)
                .bind(&ids)
                .fetch_one(&mut *tx)
                .await?;
            if ok != ids.len() as i64 {
                return Err(Error::PolicyGate("ADDON_LYRICS_REQUIRED"));
            }
            let notes = text(&i.detail, "notes", 2000)?;
            let n = ids.len() as i32;
            (
                String::new(),
                LYRIC_SYNC_PRICE,
                n,
                release,
                ids,
                format!("{rtitle} 가사 싱크"),
                json!({"notes": notes}),
            )
        }
        // Made by submit (express_from_submission), never on its own.
        _ => return Err(Error::InvalidCode("ADDON_SERVICE_INVALID")),
    };
    if !tracks.is_empty() {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND id = ANY($3)")
            .bind(org)
            .bind(release)
            .bind(&tracks)
            .fetch_one(&mut *tx)
            .await?;
        if n != tracks.len() as i64 {
            return Err(Error::InvalidCode("ADDON_TRACK_REQUIRED"));
        }
    }
    auth::rate(&s.pool, &format!("addon:{org}"), 30).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.addon_orders(id, org_id, service, tier, release_id, track_ids, title, detail, quantity, unit_price, amount, created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(id)
    .bind(org)
    .bind(&i.service)
    .bind(&tier)
    .bind(release)
    .bind(&tracks)
    .bind(title.chars().take(200).collect::<String>())
    .bind(&detail)
    .bind(quantity)
    .bind(unit)
    .bind(unit * quantity)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    event(&mut tx, id, "AWAITING_PAYMENT", "", false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "addon.created",
        &i.service,
        a.request,
    )
    .await?;
    let v = order_json(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

/// Submit makes (or keeps) the release's express order when the application
/// asks for express release, and drops an unpaid one when it no longer does.
pub async fn express_from_submission(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    draft: &Value,
    user: Uuid,
) -> Result<()> {
    let wanted = draft["options"]["express"] == true;
    if !wanted {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE portal.addon_orders SET status='CANCELLED', updated_at=now()
              WHERE org_id=$1 AND release_id=$2 AND service='EXPRESS_RELEASE' AND status='AWAITING_PAYMENT' RETURNING id",
        )
        .bind(org)
        .bind(release)
        .fetch_all(&mut *c)
        .await?;
        for id in ids {
            event(c, id, "CANCELLED", "신속 발매 없이 다시 신청", false).await?;
        }
        return Ok(());
    }
    let title = draft["title"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("발매");
    let id = Uuid::new_v4();
    let made = sqlx::query(
        "INSERT INTO portal.addon_orders(id, org_id, service, release_id, title, detail, quantity, unit_price, amount, created_by)
         VALUES($1,$2,'EXPRESS_RELEASE',$3,$4,$5,1,$6,$6,$7)
         ON CONFLICT (release_id) WHERE service = 'EXPRESS_RELEASE' AND status NOT IN ('CANCELLED','REJECTED') DO NOTHING",
    )
    .bind(id)
    .bind(org)
    .bind(release)
    .bind(title.chars().take(200).collect::<String>())
    .bind(json!({"reason": draft["options"]["expressReason"].as_str().unwrap_or(""), "release_date": draft["release_date"]}))
    .bind(EXPRESS_PRICE)
    .bind(user)
    .execute(&mut *c)
    .await?
    .rows_affected();
    if made == 1 {
        event(c, id, "AWAITING_PAYMENT", "", false).await?;
    }
    Ok(())
}

/// The org's order, locked for a payment or status change.
async fn locked(c: &mut PgConnection, org: Uuid, id: Uuid) -> Result<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT status, amount, title, pay_method, pg_order_id, pg_payment_key FROM portal.addon_orders
          WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

pub async fn pay_settlement(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    locked(&mut tx, org, id).await?;
    let r: String = sqlx::query_scalar("SELECT finance.charge_addon($1)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    match r.as_str() {
        "OK" => {}
        "BALANCE_SHORT" => return Err(Error::PolicyGate("ADDON_BALANCE_SHORT")),
        _ => return Err(Error::PolicyGate("ADDON_NOT_PAYABLE")),
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        None,
        "addon.paid",
        "SETTLEMENT",
        a.request,
    )
    .await?;
    let v = order_json(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

fn service_name(service: &str) -> &'static str {
    match service {
        "MV_DISTRIBUTION" => "뮤직비디오 배급",
        "EXPRESS_RELEASE" => "신속 발매",
        _ => "AI 가사 싱크",
    }
}

/// What the Toss payment window needs. The order id is fixed per order, so a
/// second window for the same order is the same payment.
pub async fn card_start(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let t = toss().ok_or(Error::PolicyGate("ADDON_CARD_UNAVAILABLE"))?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let row = locked(&mut tx, org, id).await?;
    if row.get::<String, _>("status") != "AWAITING_PAYMENT" {
        return Err(Error::PolicyGate("ADDON_NOT_PAYABLE"));
    }
    let pg: String = match row.get::<Option<String>, _>("pg_order_id") {
        Some(v) => v,
        None => {
            let v = format!("aq-{}", id.simple());
            sqlx::query(
                "UPDATE portal.addon_orders SET pg_order_id=$2, updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(&v)
            .execute(&mut *tx)
            .await?;
            v
        }
    };
    let service: String = sqlx::query_scalar("SELECT service FROM portal.addon_orders WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    let title: String = row.get("title");
    Ok(json!({
        "client_key": t.client_key,
        "customer_key": format!("aq-{}", a.user.simple()),
        "order_id": pg,
        "amount": row.get::<i32, _>("amount"),
        "order_name": format!("{} · {}", service_name(&service), title).chars().take(100).collect::<String>(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardConfirm {
    pub payment_key: String,
    pub order_id: String,
    pub amount: i64,
}

/// The payment window came back with a payment: confirm it with Toss for
/// exactly the order's amount, then the order is paid.
pub async fn card_confirm(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: CardConfirm,
) -> Result<Value> {
    let t = toss().ok_or(Error::PolicyGate("ADDON_CARD_UNAVAILABLE"))?;
    if i.payment_key.is_empty() || i.payment_key.len() > 200 {
        return Err(invalid());
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let row = locked(&mut tx, org, id).await?;
    let status: String = row.get("status");
    // The same payment confirmed twice (a reload of the return page).
    if row.get::<Option<String>, _>("pg_payment_key").as_deref() == Some(i.payment_key.as_str()) {
        let v = order_json(&mut tx, id).await?;
        tx.rollback().await?;
        return Ok(v);
    }
    if status != "AWAITING_PAYMENT" {
        return Err(Error::PolicyGate("ADDON_NOT_PAYABLE"));
    }
    if row.get::<Option<String>, _>("pg_order_id").as_deref() != Some(i.order_id.as_str())
        || i64::from(row.get::<i32, _>("amount")) != i.amount
    {
        return Err(Error::PolicyGate("ADDON_CARD_MISMATCH"));
    }
    let paid = toss_post(
        &t,
        "/v1/payments/confirm",
        json!({"paymentKey": i.payment_key, "orderId": i.order_id, "amount": i.amount}),
        &format!("confirm-{}", i.order_id),
    )
    .await?;
    if paid["status"] != "DONE" || paid["totalAmount"].as_i64() != Some(i.amount) {
        return Err(Error::PolicyGate("ADDON_CARD_FAILED"));
    }
    sqlx::query(
        "UPDATE portal.addon_orders SET pay_method='CARD', paid_at=now(), pg_payment_key=$2, status='SUBMITTED', updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(&i.payment_key)
    .execute(&mut *tx)
    .await?;
    event(&mut tx, id, "SUBMITTED", "카드 결제", false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        None,
        "addon.paid",
        "CARD",
        a.request,
    )
    .await?;
    let v = order_json(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

/// Close an order as CANCELLED or REJECTED and give the payment back.
/// A card payment is cancelled at Toss before anything is written, so a
/// failed card cancel leaves the order as it was.
async fn close_and_refund(
    c: &mut PgConnection,
    id: Uuid,
    status: &str,
    reason: &str,
) -> Result<()> {
    let row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT pay_method, pg_payment_key FROM portal.addon_orders WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_one(&mut *c)
    .await?;
    if row.0.as_deref() == Some("CARD") {
        let t = toss().ok_or(Error::PolicyGate("ADDON_CARD_UNAVAILABLE"))?;
        let key = row.1.unwrap_or_default();
        toss_post(
            &t,
            &format!("/v1/payments/{key}/cancel"),
            json!({"cancelReason": if reason.is_empty() { "부가서비스 취소" } else { reason }}),
            &format!("cancel-{id}"),
        )
        .await?;
    }
    sqlx::query(
        "UPDATE portal.addon_orders SET status=$2, resolution=CASE WHEN $3 <> '' THEN $3 ELSE resolution END,
                refunded_at=CASE WHEN pay_method='CARD' THEN now() ELSE refunded_at END, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(reason)
    .execute(&mut *c)
    .await?;
    sqlx::query_scalar::<_, String>("SELECT finance.refund_addon($1)")
        .bind(id)
        .fetch_one(&mut *c)
        .await?;
    Ok(())
}

/// The artist cancels until staff start the work; a paid order is refunded.
pub async fn cancel(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let row = locked(&mut tx, org, id).await?;
    let status: String = row.get("status");
    if !matches!(status.as_str(), "AWAITING_PAYMENT" | "SUBMITTED") {
        return Err(Error::PolicyGate("ADDON_NOT_CANCELLABLE"));
    }
    close_and_refund(&mut tx, id, "CANCELLED", "").await?;
    event(&mut tx, id, "CANCELLED", "", false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        None,
        "addon.cancelled",
        &status,
        a.request,
    )
    .await?;
    let v = order_json(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// Synced lyrics (LRC)
// ---------------------------------------------------------------------------
pub async fn lyric_sync(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',t.id,'title',t.title,'disc_number',t.disc_number,'track_number',t.track_number,
                'has_lyrics',length(btrim(coalesce(t.lyrics,''))) > 0,
                'lrc',l.lrc,'source',l.source,'registered',COALESCE(to_jsonb(l.registered),'[]'::jsonb),'updated_at',l.updated_at)
           FROM catalog.tracks t LEFT JOIN portal.track_lyric_sync l ON l.track_id=t.id
          WHERE t.org_id=$1 AND t.release_id=$2 ORDER BY t.disc_number, t.track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LrcInput {
    pub lrc: String,
}

/// The artist's own synced lyrics for a track (free).
pub async fn put_lyric_sync(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    track: Uuid,
    i: LrcInput,
) -> Result<Value> {
    let lines = lrc_lines(&i.lrc)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND id=$3)",
    )
    .bind(org)
    .bind(release)
    .bind(track)
    .fetch_one(&mut *tx)
    .await?;
    if !ok {
        return Err(Error::NotFound);
    }
    sqlx::query(
        "INSERT INTO portal.track_lyric_sync(track_id, org_id, release_id, lrc, source, updated_by) VALUES($1,$2,$3,$4,'ARTIST',$5)
         ON CONFLICT (track_id) DO UPDATE SET lrc=$4, source='ARTIST', order_id=NULL, registered='{}', updated_by=$5, updated_at=now()",
    )
    .bind(track)
    .bind(org)
    .bind(release)
    .bind(i.lrc.trim())
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "lyric_sync.uploaded",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"track_id": track, "lines": lines}))
}

// ---------------------------------------------------------------------------
// Staff
// ---------------------------------------------------------------------------
#[derive(Deserialize, Default)]
pub struct StaffQuery {
    /// "open" (default) or "all".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub service: String,
}

pub async fn staff_list(s: &AppState, h: &HeaderMap, q: StaffQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {ORDER_JSON} || jsonb_build_object('org_name',g.name,'pg_order_id',o.pg_order_id) FROM portal.addon_orders o
           JOIN identity.orgs g ON g.id=o.org_id
          WHERE ($1 OR o.status IN ('AWAITING_PAYMENT','SUBMITTED','IN_PROGRESS','DELIVERED'))
            AND ($2 = '' OR o.service = $2)
          ORDER BY (o.status IN ('SUBMITTED','IN_PROGRESS','DELIVERED')) DESC,
                   (o.service='EXPRESS_RELEASE' OR o.tier='EXPRESS') DESC, o.created_at LIMIT 300"
    )))
    .bind(q.status == "all")
    .bind(q.service.as_str())
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffUpdate {
    pub status: String,
    #[serde(default)]
    pub note: String,
    /// What the work produced (rating, rating number, links per platform…).
    #[serde(default)]
    pub result: Option<Value>,
}

/// Staff result fields, checked: short strings, a rating, https links per
/// known platform.
fn staff_result(v: &Value) -> Result<Value> {
    let o = v.as_object().ok_or_else(invalid)?;
    let mut out = Map::new();
    for (k, val) in o {
        match k.as_str() {
            "rating" => {
                let r = val.as_str().unwrap_or("");
                if !r.is_empty() && !RATINGS.contains(&r) {
                    return Err(invalid());
                }
                out.insert(k.clone(), json!(r));
            }
            "rating_no" | "note" => {
                let t = val.as_str().unwrap_or("").trim();
                staff::note_ok(t, 500)?;
                out.insert(k.clone(), json!(t));
            }
            "links" => {
                let links = val.as_object().ok_or_else(invalid)?;
                let mut m = Map::new();
                for (p, u) in links {
                    if !MV_PLATFORMS.contains(&p.as_str()) && !LYRIC_SERVICES.contains(&p.as_str())
                    {
                        return Err(invalid());
                    }
                    let u = u.as_str().unwrap_or("").trim();
                    if !u.is_empty() {
                        m.insert(p.clone(), json!(https_url(u)?));
                    }
                }
                out.insert(k.clone(), Value::Object(m));
            }
            _ => return Err(invalid()),
        }
    }
    Ok(Value::Object(out))
}

pub async fn staff_update(s: &AppState, h: &HeaderMap, id: Uuid, i: StaffUpdate) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(&i.note, 1000)?;
    let result = i.result.as_ref().map(staff_result).transpose()?;
    let from: &[&str] = match i.status.as_str() {
        "IN_PROGRESS" => &["SUBMITTED"],
        "DELIVERED" => &["SUBMITTED", "IN_PROGRESS"],
        "COMPLETED" => &["SUBMITTED", "IN_PROGRESS", "DELIVERED"],
        "REJECTED" => &["AWAITING_PAYMENT", "SUBMITTED", "IN_PROGRESS", "DELIVERED"],
        // Same status: only the result/note changes.
        "UPDATE" => OPEN,
        _ => return Err(Error::InvalidCode("ADDON_STATUS_INVALID")),
    };
    if i.status == "REJECTED" && i.note.trim().is_empty() {
        return Err(Error::InvalidCode("ADDON_NOTE_REQUIRED"));
    }
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row: (Uuid, Option<Uuid>, String) = sqlx::query_as(
        "SELECT org_id, release_id, status FROM portal.addon_orders WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if !from.contains(&row.2.as_str()) {
        return Err(Error::Conflict);
    }
    if let Some(r) = &result {
        sqlx::query(
            "UPDATE portal.addon_orders SET result = result || $2, updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(r)
        .execute(&mut *tx)
        .await?;
    }
    if i.status == "REJECTED" {
        close_and_refund(&mut tx, id, "REJECTED", i.note.trim()).await?;
        event(&mut tx, id, "REJECTED", i.note.trim(), true).await?;
    } else if i.status != "UPDATE" {
        sqlx::query(
            "UPDATE portal.addon_orders SET status=$2,
                    resolution=CASE WHEN $3 <> '' AND $2 IN ('DELIVERED','COMPLETED') THEN $3 ELSE resolution END, updated_at=now()
              WHERE id=$1",
        )
        .bind(id)
        .bind(&i.status)
        .bind(i.note.trim())
        .execute(&mut *tx)
        .await?;
        event(&mut tx, id, &i.status, i.note.trim(), true).await?;
    }
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(row.0),
        row.1,
        "staff.addon",
        &i.status,
        st.actor.request,
    )
    .await?;
    let v = order_json(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

/// The order's tracks with their lyrics and any synced lyrics, for the staff
/// member making the LRC.
pub async fn staff_lyrics(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',t.id,'title',t.title,'lyrics',coalesce(t.lyrics,''),'lrc',l.lrc,'source',l.source,
                'registered',COALESCE(to_jsonb(l.registered),'[]'::jsonb))
           FROM portal.addon_orders o JOIN catalog.tracks t ON t.id = ANY(o.track_ids)
           LEFT JOIN portal.track_lyric_sync l ON l.track_id=t.id
          WHERE o.id=$1 ORDER BY t.disc_number, t.track_number",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items, "services": LYRIC_SERVICES}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffLrc {
    pub lrc: String,
    #[serde(default)]
    pub registered: Vec<String>,
}

/// Staff deliver the synced lyrics made for a LYRIC_SYNC order's track and
/// record where they are registered.
pub async fn staff_put_lyrics(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    track: Uuid,
    i: StaffLrc,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    let lines = lrc_lines(&i.lrc)?;
    if i.registered.len() > LYRIC_SERVICES.len()
        || i.registered
            .iter()
            .any(|r| !LYRIC_SERVICES.contains(&r.as_str()))
    {
        return Err(invalid());
    }
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row: (Uuid, Option<Uuid>) = sqlx::query_as(
        "SELECT org_id, release_id FROM portal.addon_orders WHERE id=$1 AND service='LYRIC_SYNC' AND $2 = ANY(track_ids)
            AND status IN ('SUBMITTED','IN_PROGRESS','DELIVERED','COMPLETED')",
    )
    .bind(id)
    .bind(track)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let release = row.1.ok_or(Error::NotFound)?;
    sqlx::query(
        "INSERT INTO portal.track_lyric_sync(track_id, org_id, release_id, lrc, source, order_id, registered, updated_by)
         VALUES($1,$2,$3,$4,'AI',$5,$6,$7)
         ON CONFLICT (track_id) DO UPDATE SET lrc=$4, source='AI', order_id=$5, registered=$6, updated_by=$7, updated_at=now()",
    )
    .bind(track)
    .bind(row.0)
    .bind(release)
    .bind(i.lrc.trim())
    .bind(id)
    .bind(&i.registered)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(row.0),
        Some(release),
        "staff.lyric_sync",
        "AI",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"track_id": track, "lines": lines}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list(&s, &a, org).await?))
}
async fn h_create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<NewOrder>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(create(&s, &a, org, i).await?))
}
async fn h_pay_settlement(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(pay_settlement(&s, &a, org, id).await?))
}
async fn h_card_start(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(card_start(&s, &a, org, id).await?))
}
async fn h_card_confirm(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<CardConfirm>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(card_confirm(&s, &a, org, id, i).await?))
}
async fn h_cancel(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel(&s, &a, org, id).await?))
}
async fn h_lyric_sync(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(lyric_sync(&s, &a, org, id).await?))
}
async fn h_put_lyric_sync(
    State(s): State<AppState>,
    Path((org, release, track)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<LrcInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(put_lyric_sync(&s, &a, org, release, track, i).await?))
}
async fn h_staff_list(
    State(s): State<AppState>,
    Query(q): Query<StaffQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_list(&s, &h, q).await?))
}
async fn h_staff_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<StaffUpdate>,
) -> Result<Json<Value>> {
    Ok(Json(staff_update(&s, &h, id, i).await?))
}
async fn h_staff_lyrics(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_lyrics(&s, &h, id).await?))
}
async fn h_staff_put_lyrics(
    State(s): State<AppState>,
    Path((id, track)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<StaffLrc>,
) -> Result<Json<Value>> {
    Ok(Json(staff_put_lyrics(&s, &h, id, track, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/addons", get(h_list).post(h_create))
        .route(
            "/api/orgs/{org}/addons/{id}/pay/settlement",
            post(h_pay_settlement),
        )
        .route("/api/orgs/{org}/addons/{id}/pay/card", post(h_card_start))
        .route(
            "/api/orgs/{org}/addons/{id}/pay/card/confirm",
            post(h_card_confirm),
        )
        .route("/api/orgs/{org}/addons/{id}/cancel", post(h_cancel))
        .route(
            "/api/orgs/{org}/releases/{id}/lyric-sync",
            get(h_lyric_sync),
        )
        .route(
            "/api/orgs/{org}/releases/{release}/tracks/{track}/lyric-sync",
            put(h_put_lyric_sync),
        )
        .route("/api/staff/addons", get(h_staff_list))
        .route("/api/staff/addons/{id}/status", post(h_staff_update))
        .route("/api/staff/addons/{id}/lyrics", get(h_staff_lyrics))
        .route(
            "/api/staff/addons/{id}/lyrics/{track}",
            put(h_staff_put_lyrics),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_accepts_synced_lyrics_only() {
        let ok =
            "[ti:여름의 끝]\n[ar:서린]\n\n[00:12.30]첫 줄\n[00:15.00][01:02.50]후렴\n[03:01]끝";
        assert_eq!(lrc_lines(ok).unwrap(), 3);
        for bad in [
            "",
            "그냥 가사",
            "[00:12.30]첫 줄\n시간 없는 줄",
            "[00:75.00]초가 60 넘음",
            "[12:3]짧음",
            "[ti:제목만]",
        ] {
            assert!(lrc_lines(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn mv_details_follow_the_tier() {
        let base = json!({"video_url": "https://drive.example.com/mv.mp4", "mv_title": "여름의 끝 MV", "duration": "3:45", "platforms": ["melon", "youtube"]});
        assert!(mv_detail("REVIEW", &base).is_ok());
        // Distribution only needs the rating it already has.
        assert!(mv_detail("BASIC", &base).is_err());
        let mut rated = base.clone();
        rated["rating"] = json!("15");
        rated["rating_no"] = json!("제2026-MV0001호");
        assert_eq!(mv_detail("BASIC", &rated).unwrap()["rating"], "15");
        let mut bad = base.clone();
        bad["platforms"] = json!(["netflix"]);
        assert!(mv_detail("REVIEW", &bad).is_err());
        let mut http = base;
        http["video_url"] = json!("http://drive.example.com/mv.mp4");
        assert!(mv_detail("REVIEW", &http).is_err());
    }
}
