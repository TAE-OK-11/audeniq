//! Add-on services (migration 0157): paid work chosen in the release
//! application, paid at its end from the settlement balance or by card, and
//! worked by staff to the end.
//!
//! - `MV_DISTRIBUTION`: a music video to the major video DSPs. `BASIC`
//!   (already rated by 영상물등급위원회: distribution only), `REVIEW`
//!   (+ rating filed for the artist), `EXPRESS` (+ expedited handling). The
//!   video is a link or a file uploaded in parts ([`video_start`]).
//! - `EXPRESS_RELEASE`: the release is reviewed and delivered first so it can
//!   go out three business days after submission.
//! - `LYRIC_SYNC`: time-synced lyrics (LRC) made for the chosen tracks and
//!   registered with lyric services, priced per track. An artist may also
//!   upload their own LRC for free ([`put_lyric_sync`]).
//!
//! Submit makes the orders from the application's options
//! ([`sync_from_submission`]). [`pay`] pays all of a release's unpaid orders
//! at once: from the balance through `finance.charge_addon` (the API cannot
//! write the ledger; the function locks like payouts and refuses an amount
//! over the withdrawable balance), or by card through the configured PG
//! ([`crate::payments`]) with [`confirm`] checking the amount server-side.
//! A cancelled (artist, before work starts) or rejected (staff) paid order is
//! refunded: `finance.refund_addon` for the balance, that order's part of the
//! card payment at the PG.
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
use sqlx::PgConnection;
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
// Orders
// ---------------------------------------------------------------------------
const ORDER_JSON: &str = "jsonb_build_object('id',o.id,'service',o.service,'tier',o.tier,'release_id',o.release_id,
    'release_title',(SELECT r.title FROM catalog.releases r WHERE r.id=o.release_id),'track_ids',o.track_ids,
    'tracks',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',t.id,'title',t.title) ORDER BY t.disc_number,t.track_number)
                       FROM catalog.tracks t WHERE t.id = ANY(o.track_ids)),'[]'::jsonb),
    'video',(SELECT jsonb_build_object('id',f.id,'name',f.name,'size_bytes',f.size_bytes,'checks',f.video_checks) FROM portal.addon_files f
              WHERE f.org_id=o.org_id AND f.id::text = o.detail->>'video_file'),
    'title',o.title,'detail',o.detail,'quantity',o.quantity,'unit_price',o.unit_price,'amount',o.amount,
    'status',o.status,'pay_method',o.pay_method,'paid_at',o.paid_at,'refunded_at',o.refunded_at,
    'provider',(SELECT p.provider FROM portal.addon_payments p WHERE p.id=o.payment_id),
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

fn balance_won(d: rust_decimal::Decimal) -> i64 {
    d.trunc().to_string().parse::<i64>().unwrap_or(0).max(0)
}

pub fn prices() -> Value {
    json!({
        "mv": MV_TIERS.iter().map(|(t, p)| json!({"tier": t, "price": p})).collect::<Vec<_>>(),
        "express_release": EXPRESS_PRICE,
        "lyric_sync": LYRIC_SYNC_PRICE,
    })
}

fn card() -> Value {
    crate::payments::provider()
        .map(|p| json!({"provider": p.kind.name(), "label": p.card_label()}))
        .unwrap_or(Value::Null)
}

/// What the application needs before there is a release to pay for:
/// prices, the card provider (or none) and the balance that can pay.
pub async fn config(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let balance = crate::finance::withdrawable(&mut tx, org, None).await?;
    tx.rollback().await?;
    Ok(
        json!({"prices": prices(), "card": card(), "balance": balance_won(balance), "mv_platforms": MV_PLATFORMS}),
    )
}

/// The release's add-on orders with what paying them needs.
pub async fn release_orders(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {ORDER_JSON} FROM portal.addon_orders o WHERE o.org_id=$1 AND o.release_id=$2 ORDER BY o.created_at"
    )))
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let balance = crate::finance::withdrawable(&mut tx, org, None).await?;
    tx.rollback().await?;
    Ok(json!({"items": items, "prices": prices(), "card": card(), "balance": balance_won(balance)}))
}

/// One wanted order, worked out from the application.
struct Wanted {
    tier: String,
    unit: i32,
    quantity: i32,
    tracks: Vec<Uuid>,
    title: String,
    detail: Value,
}

/// Tracks of the release in order (disc, number): the application names
/// them by position (1-based) because its own track ids are client-side.
async fn tracks_by_position(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
) -> Result<Vec<(Uuid, String, bool)>> {
    Ok(sqlx::query_as(
        "SELECT id, title, length(btrim(coalesce(lyrics,''))) > 0 FROM catalog.tracks
          WHERE org_id=$1 AND release_id=$2 ORDER BY disc_number, track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *c)
    .await?)
}

/// Validated music video details from the application's options.
/// An uploaded video and what the server read from its header.
struct VideoFile {
    id: Uuid,
    duration: String,
    width: Option<i32>,
    height: Option<i32>,
}

fn mv_detail(tier: &str, o: &Value, video_file: Option<VideoFile>) -> Result<Value> {
    let url = o
        .get("mvVideoUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let video_url = if url.is_empty() {
        String::new()
    } else {
        https_url(url)?
    };
    if video_file.is_none() && video_url.is_empty() {
        return Err(Error::InvalidCode("ADDON_MV_VIDEO_REQUIRED"));
    }
    let title = text(o, "mvTitle", 200)?;
    if title.is_empty() {
        return Err(Error::InvalidCode("ADDON_MV_TITLE_REQUIRED"));
    }
    let platforms: Vec<String> = o
        .get("mvPlatforms")
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
    out.insert("video_url".into(), json!(video_url));
    // Length and size come from the uploaded file's header, never from the
    // form; a linked video is measured by staff when they download it.
    let mut duration = String::new();
    if let Some(f) = video_file {
        out.insert("video_file".into(), json!(f.id.to_string()));
        duration = f.duration;
        if let (Some(w), Some(h)) = (f.width, f.height) {
            out.insert("width".into(), json!(w));
            out.insert("height".into(), json!(h));
        }
    }
    out.insert("mv_title".into(), json!(title));
    out.insert("duration".into(), json!(duration));
    out.insert("platforms".into(), json!(platforms));
    for (k, key, max) in [
        ("director", "mvDirector", 200),
        ("production", "mvProduction", 200),
        ("cast", "mvCast", 500),
        ("synopsis", "mvSynopsis", 2000),
    ] {
        out.insert(k.into(), json!(text(o, key, max)?));
    }
    let date = text(o, "mvDate", 10)?;
    if !date.is_empty() && chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err(invalid());
    }
    out.insert("release_date".into(), json!(date));
    if tier == "BASIC" {
        // Already rated: the rating and its number go with the delivery.
        let rating = text(o, "mvRating", 3)?;
        let rating_no = text(o, "mvRatingNo", 60)?;
        if !RATINGS.contains(&rating.as_str()) || rating_no.is_empty() {
            return Err(Error::InvalidCode("ADDON_MV_RATING_REQUIRED"));
        }
        out.insert("rating".into(), json!(rating));
        out.insert("rating_no".into(), json!(rating_no));
    }
    Ok(Value::Object(out))
}

/// The orders the application asks for, checked against the release.
async fn wanted(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    draft: &Value,
) -> Result<Vec<(&'static str, Option<Wanted>)>> {
    let o = &draft["options"];
    let on = |k: &str| o[k] == true;
    let title = draft["title"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("발매")
        .chars()
        .take(150)
        .collect::<String>();
    let tracks = tracks_by_position(c, org, release).await?;
    let pick = |n: &Value| -> Option<&(Uuid, String, bool)> {
        let i = n.as_u64()? as usize;
        tracks.get(i.checked_sub(1)?)
    };
    let express = on("express").then(|| Wanted {
        tier: String::new(),
        unit: EXPRESS_PRICE,
        quantity: 1,
        tracks: vec![],
        title: title.clone(),
        detail: json!({"reason": o["expressReason"].as_str().unwrap_or("").chars().take(300).collect::<String>(), "release_date": draft["release_date"]}),
    });
    let mv = if on("mv") {
        let tier = o["mvTier"].as_str().unwrap_or("");
        let unit = MV_TIERS
            .iter()
            .find(|(t, _)| *t == tier)
            .map(|(_, p)| *p)
            .ok_or(Error::InvalidCode("ADDON_TIER_INVALID"))?;
        let track = pick(&o["mvTrack"]).ok_or(Error::InvalidCode("ADDON_TRACK_REQUIRED"))?;
        let file = match o["mvVideoFile"].as_str().filter(|v| !v.is_empty()) {
            Some(v) => {
                let id = Uuid::parse_str(v)
                    .map_err(|_| Error::InvalidCode("ADDON_MV_VIDEO_REQUIRED"))?;
                let probed: Option<(Option<i64>, Option<i32>, Option<i32>)> = sqlx::query_as("SELECT duration_ms, width, height FROM portal.addon_files WHERE org_id=$1 AND id=$2 AND status='READY'")
                    .bind(org)
                    .bind(id)
                    .fetch_optional(&mut *c)
                    .await?;
                let Some((ms, w, h)) = probed else {
                    return Err(Error::PolicyGate("ADDON_MV_VIDEO_NOT_READY"));
                };
                Some(VideoFile {
                    id,
                    duration: video_clock(ms.unwrap_or(0)),
                    width: w,
                    height: h,
                })
            }
            None => None,
        };
        let detail = mv_detail(tier, o, file)?;
        let mv_title = detail["mv_title"].as_str().unwrap_or("").to_string();
        Some(Wanted {
            tier: tier.into(),
            unit,
            quantity: 1,
            tracks: vec![track.0],
            title: mv_title,
            detail,
        })
    } else {
        None
    };
    let lyric = if on("lyricSync") {
        let mut ids: Vec<Uuid> = Vec::new();
        for n in o["lyricTracks"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let t = pick(n).ok_or(Error::InvalidCode("ADDON_TRACK_REQUIRED"))?;
            if !t.2 {
                return Err(Error::PolicyGate("ADDON_LYRICS_REQUIRED"));
            }
            if !ids.contains(&t.0) {
                ids.push(t.0);
            }
        }
        if ids.is_empty() {
            return Err(Error::InvalidCode("ADDON_TRACK_REQUIRED"));
        }
        let n = ids.len() as i32;
        Some(Wanted {
            tier: String::new(),
            unit: LYRIC_SYNC_PRICE,
            quantity: n,
            tracks: ids,
            title: format!("{title} 가사 싱크"),
            detail: json!({}),
        })
    } else {
        None
    };
    Ok(vec![
        ("EXPRESS_RELEASE", express),
        ("MV_DISTRIBUTION", mv),
        ("LYRIC_SYNC", lyric),
    ])
}

/// Submit turns the application's add-on choices into orders: a new choice
/// makes an order waiting for payment, a changed one updates the unpaid
/// order, a dropped one cancels the unpaid order. A paid order is kept as it
/// was (changes to it go through staff).
pub async fn sync_from_submission(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    draft: &Value,
    user: Uuid,
) -> Result<()> {
    for (service, want) in wanted(c, org, release, draft).await? {
        let live: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT id, status FROM portal.addon_orders WHERE release_id=$1 AND service=$2 AND status NOT IN ('CANCELLED','REJECTED') FOR UPDATE",
        )
        .bind(release)
        .bind(service)
        .fetch_optional(&mut *c)
        .await?;
        match (want, live) {
            (None, Some((id, st))) if st == "AWAITING_PAYMENT" => {
                sqlx::query("UPDATE portal.addon_orders SET status='CANCELLED', updated_at=now() WHERE id=$1").bind(id).execute(&mut *c).await?;
                event(c, id, "CANCELLED", "발매 신청에서 뺐어요", false).await?;
            }
            (Some(w), Some((id, st))) if st == "AWAITING_PAYMENT" => {
                sqlx::query(
                    "UPDATE portal.addon_orders SET tier=$2, track_ids=$3, title=$4, detail=$5, quantity=$6, unit_price=$7, amount=$8, updated_at=now() WHERE id=$1",
                )
                .bind(id)
                .bind(&w.tier)
                .bind(&w.tracks)
                .bind(&w.title)
                .bind(&w.detail)
                .bind(w.quantity)
                .bind(w.unit)
                .bind(w.unit * w.quantity)
                .execute(&mut *c)
                .await?;
            }
            (Some(w), None) => {
                let id = Uuid::new_v4();
                sqlx::query(
                    "INSERT INTO portal.addon_orders(id, org_id, service, tier, release_id, track_ids, title, detail, quantity, unit_price, amount, created_by)
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
                )
                .bind(id)
                .bind(org)
                .bind(service)
                .bind(&w.tier)
                .bind(release)
                .bind(&w.tracks)
                .bind(&w.title)
                .bind(&w.detail)
                .bind(w.quantity)
                .bind(w.unit)
                .bind(w.unit * w.quantity)
                .bind(user)
                .execute(&mut *c)
                .await?;
                event(c, id, "AWAITING_PAYMENT", "", false).await?;
            }
            // Paid (or nothing wanted and nothing live): leave it.
            _ => {}
        }
    }
    Ok(())
}

fn service_name(service: &str) -> &'static str {
    match service {
        "MV_DISTRIBUTION" => "뮤직비디오 배급",
        "EXPRESS_RELEASE" => "신속 발매",
        _ => "AI 가사 싱크",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayInput {
    /// SETTLEMENT or CARD.
    pub method: String,
    /// Which of the release's unpaid orders; all of them when empty.
    #[serde(default)]
    pub order_ids: Vec<Uuid>,
}

/// Pay the release's unpaid add-ons at once. From the balance: every order
/// is charged in one transaction (all or none). By card: a payment for
/// their total is opened and the window data returned; [`confirm`] finishes.
pub async fn pay(s: &AppState, a: &Actor, org: Uuid, release: Uuid, i: PayInput) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let orders: Vec<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT id, amount, service FROM portal.addon_orders
          WHERE org_id=$1 AND release_id=$2 AND status='AWAITING_PAYMENT' AND (cardinality($3::uuid[])=0 OR id = ANY($3))
          ORDER BY created_at FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .bind(&i.order_ids)
    .fetch_all(&mut *tx)
    .await?;
    if orders.is_empty() || (!i.order_ids.is_empty() && orders.len() != i.order_ids.len()) {
        return Err(Error::PolicyGate("ADDON_NOT_PAYABLE"));
    }
    match i.method.as_str() {
        "SETTLEMENT" => {
            for (id, _, _) in &orders {
                let r: String = sqlx::query_scalar("SELECT finance.charge_addon($1)")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await?;
                match r.as_str() {
                    "OK" => {}
                    "BALANCE_SHORT" => return Err(Error::PolicyGate("ADDON_BALANCE_SHORT")),
                    _ => return Err(Error::PolicyGate("ADDON_NOT_PAYABLE")),
                }
            }
            operations::audit(
                &mut tx,
                Some(a.user),
                Some(org),
                Some(release),
                "addon.paid",
                "SETTLEMENT",
                a.request,
            )
            .await?;
            tx.commit().await?;
            release_orders(s, a, org, release).await
        }
        "CARD" => {
            let p =
                crate::payments::provider().ok_or(Error::PolicyGate("ADDON_CARD_UNAVAILABLE"))?;
            auth::rate(&s.pool, &format!("addon-card:{org}"), 30).await?;
            let total: i64 = orders.iter().map(|o| i64::from(o.1)).sum();
            let id = Uuid::new_v4();
            let pg = format!("aq{}", id.simple());
            let ids: Vec<Uuid> = orders.iter().map(|o| o.0).collect();
            sqlx::query(
                "INSERT INTO portal.addon_payments(id, org_id, release_id, order_ids, amount, provider, pg_order_id, created_by)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(id)
            .bind(org)
            .bind(release)
            .bind(&ids)
            .bind(total as i32)
            .bind(p.kind.name())
            .bind(&pg)
            .bind(a.user)
            .execute(&mut *tx)
            .await?;
            let rtitle: String = sqlx::query_scalar(
                "SELECT coalesce(nullif(btrim(title),''),'발매') FROM catalog.releases WHERE id=$1",
            )
            .bind(release)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            let what = if orders.len() == 1 {
                service_name(&orders[0].2).to_string()
            } else {
                format!("부가서비스 {}건", orders.len())
            };
            let name: String = format!("{what} · {rtitle}").chars().take(100).collect();
            let mut w = p.window(
                &pg,
                total,
                &name,
                &format!("aq-{}", a.user.simple()),
                &s.config.origin,
            );
            w["payment_id"] = json!(id);
            Ok(w)
        }
        _ => Err(Error::InvalidCode("ADDON_PAY_METHOD_INVALID")),
    }
}

/// The payment window came back: confirm with the PG for exactly the
/// payment's total, then its orders are paid. A repeated confirm of a paid
/// payment returns the orders as they are.
pub async fn confirm(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    payment: Uuid,
    input: Value,
) -> Result<Value> {
    let p = crate::payments::provider().ok_or(Error::PolicyGate("ADDON_CARD_UNAVAILABLE"))?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let row: (Option<Uuid>, Vec<Uuid>, i32, String, String, String) = sqlx::query_as(
        "SELECT release_id, order_ids, amount, provider, pg_order_id, status FROM portal.addon_payments WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(payment)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let (release, order_ids, amount, provider, pg, status) = row;
    let release = release.ok_or(Error::NotFound)?;
    if status == "PAID" {
        tx.rollback().await?;
        return release_orders(s, a, org, release).await;
    }
    if status != "PENDING" || provider != p.kind.name() {
        return Err(Error::PolicyGate("ADDON_NOT_PAYABLE"));
    }
    let orders: Vec<(Uuid, i32, String)> =
        sqlx::query_as("SELECT id, amount, status FROM portal.addon_orders WHERE id = ANY($1) AND org_id=$2 FOR UPDATE")
            .bind(&order_ids)
            .bind(org)
            .fetch_all(&mut *tx)
            .await?;
    let paid = p
        .confirm(&pg, i64::from(amount), &input, &s.config.origin)
        .await?;
    sqlx::query("UPDATE portal.addon_payments SET status='PAID', pg_key=$2, pg_date=NULLIF($3,''), paid_at=now() WHERE id=$1")
        .bind(payment)
        .bind(&paid.key)
        .bind(&paid.date)
        .execute(&mut *tx)
        .await?;
    for (id, part, st) in orders {
        if st == "AWAITING_PAYMENT" {
            sqlx::query(
                "UPDATE portal.addon_orders SET pay_method='CARD', paid_at=now(), payment_id=$2, status='SUBMITTED', updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(payment)
            .execute(&mut *tx)
            .await?;
            event(
                &mut tx,
                id,
                "SUBMITTED",
                &format!("카드 결제 ({})", p.card_label()),
                false,
            )
            .await?;
        } else {
            // Cancelled or paid another way while the window was open: give that part back.
            p.cancel(
                &paid,
                i64::from(part),
                "이미 처리된 신청",
                &format!("extra-{id}"),
                &s.config.origin,
            )
            .await?;
        }
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "addon.paid",
        "CARD",
        a.request,
    )
    .await?;
    tx.commit().await?;
    release_orders(s, a, org, release).await
}

/// Close an order as CANCELLED or REJECTED and give its payment back. A
/// card part is cancelled at the PG before anything is written, so a failed
/// cancel leaves the order as it was.
async fn close_and_refund(
    c: &mut PgConnection,
    origin: &str,
    id: Uuid,
    status: &str,
    reason: &str,
) -> Result<()> {
    let row: (Option<String>, i32, Option<Uuid>) = sqlx::query_as(
        "SELECT pay_method, amount, payment_id FROM portal.addon_orders WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_one(&mut *c)
    .await?;
    if row.0.as_deref() == Some("CARD") {
        let (provider, key, date): (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT provider, pg_key, pg_date FROM portal.addon_payments WHERE id=$1",
        )
        .bind(row.2)
        .fetch_one(&mut *c)
        .await?;
        let p = crate::payments::provider()
            .filter(|p| p.kind.name() == provider)
            .ok_or(Error::PolicyGate("ADDON_CARD_UNAVAILABLE"))?;
        let paid = crate::payments::Paid {
            key: key.unwrap_or_default(),
            date: date.unwrap_or_default(),
        };
        p.cancel(
            &paid,
            i64::from(row.1),
            reason,
            &format!("cancel-{id}"),
            origin,
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
    let (status, release): (String, Uuid) = sqlx::query_as(
        "SELECT status, release_id FROM portal.addon_orders WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    if !matches!(status.as_str(), "AWAITING_PAYMENT" | "SUBMITTED") {
        return Err(Error::PolicyGate("ADDON_NOT_CANCELLABLE"));
    }
    close_and_refund(&mut tx, &s.config.origin, id, "CANCELLED", "").await?;
    event(&mut tx, id, "CANCELLED", "", false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "addon.cancelled",
        &status,
        a.request,
    )
    .await?;
    tx.commit().await?;
    release_orders(s, a, org, release).await
}

/// Payple posts the certification result to this address too; the Studio
/// page reads it from the window's callback, so this only closes the layer.
async fn h_payple_return() -> axum::response::Html<&'static str> {
    axum::response::Html(
        "<!doctype html><meta charset=utf-8><title>AUDENIQ</title><p style=\"font:14px sans-serif;text-align:center;margin-top:40vh\">결제 결과를 확인하고 있어요. 이 창은 닫아도 돼요.</p>",
    )
}

// ---------------------------------------------------------------------------
// Music video files (multipart, straight to storage)
// ---------------------------------------------------------------------------
/// Largest music video (a server-side store copy is never needed: the
/// upload lands at its final key).
pub const MAX_VIDEO_BYTES: i64 = 8 * 1024 * 1024 * 1024;
const VIDEO_PART: i64 = crate::uploads::PART_SIZE;

fn video_parts(size: i64) -> i64 {
    (size + VIDEO_PART - 1) / VIDEO_PART
}
fn video_part_len(size: i64, n: i64) -> i64 {
    (size - (n - 1) * VIDEO_PART).min(VIDEO_PART)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoInput {
    pub name: String,
    pub size_bytes: i64,
    pub content_type: String,
}

pub async fn video_start(s: &AppState, a: &Actor, org: Uuid, i: VideoInput) -> Result<Value> {
    if !matches!(i.content_type.as_str(), "video/mp4" | "video/quicktime") {
        return Err(Error::InvalidCode("ADDON_VIDEO_TYPE_INVALID"));
    }
    if !(1..=MAX_VIDEO_BYTES).contains(&i.size_bytes) {
        return Err(Error::InvalidCode("ADDON_VIDEO_TOO_LARGE"));
    }
    let name: String = i
        .name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    if name.is_empty() {
        return Err(invalid());
    }
    auth::membership(&mut *s.pool.acquire().await?, a, org, true).await?;
    auth::rate(&s.pool, &format!("addon-video:{}", a.user), 20).await?;
    let id = Uuid::new_v4();
    let key = format!("addon-video/{org}/{id}");
    let upload = s
        .storage
        .create_multipart(&key, &i.content_type, &id.to_string())
        .await?;
    sqlx::query(
        "INSERT INTO portal.addon_files(id, org_id, object_key, name, size_bytes, content_type, upload_id, created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(org)
    .bind(&key)
    .bind(&name)
    .bind(i.size_bytes)
    .bind(&i.content_type)
    .bind(&upload)
    .bind(a.user)
    .execute(&s.pool)
    .await?;
    Ok(json!({"id": id, "part_size": VIDEO_PART, "part_count": video_parts(i.size_bytes)}))
}

struct OpenVideo {
    key: String,
    upload: String,
    size: i64,
}

async fn open_video(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<OpenVideo> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let row: (String, String, i64, String) = sqlx::query_as(
        "SELECT object_key, upload_id, size_bytes, status FROM portal.addon_files WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    tx.rollback().await?;
    if row.3 != "UPLOADING" {
        return Err(Error::Conflict);
    }
    Ok(OpenVideo {
        key: row.0,
        upload: row.1,
        size: row.2,
    })
}

/// Parts already stored (where a resumed upload continues).
pub async fn video_stored(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let v = open_video(s, a, org, id).await?;
    let count = video_parts(v.size);
    let parts: Vec<Value> = s
        .storage
        .list_parts(&v.key, &v.upload)
        .await?
        .iter()
        .filter(|p| {
            (1..=count).contains(&i64::from(p.number))
                && p.size == video_part_len(v.size, i64::from(p.number))
        })
        .map(|p| json!({"part_number": p.number, "size_bytes": p.size}))
        .collect();
    Ok(json!({"part_size": VIDEO_PART, "part_count": count, "parts": parts}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoParts {
    pub part_numbers: Vec<i32>,
}

pub async fn video_part_urls(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: VideoParts,
) -> Result<Value> {
    if i.part_numbers.is_empty() || i.part_numbers.len() > 20 {
        return Err(Error::InvalidCode("UPLOAD_PARTS_INVALID"));
    }
    auth::rate(&s.pool, &format!("addon-video-parts:{}", a.user), 3000).await?;
    let v = open_video(s, a, org, id).await?;
    let count = video_parts(v.size);
    let expires = chrono::Utc::now() + chrono::Duration::minutes(15);
    let mut parts = Vec::with_capacity(i.part_numbers.len());
    for n in i.part_numbers {
        if !(1..=count).contains(&i64::from(n)) {
            return Err(Error::InvalidCode("UPLOAD_PARTS_INVALID"));
        }
        let size = video_part_len(v.size, i64::from(n));
        let g = s
            .storage
            .presign_part(&v.key, &v.upload, n, size, expires)
            .await?;
        parts.push(json!({"part_number": n, "size_bytes": size, "grant": g}));
    }
    Ok(json!({"parts": parts}))
}

/// Every part stored at its size → the object is assembled, its size and
/// container (MP4/MOV `ftyp` box) checked; anything else is removed. The
/// media engine then reports codecs, picture, sound and delivery issues
/// ([`crate::video`]), shown to the artist and staff.
pub async fn video_complete(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let v = open_video(s, a, org, id).await?;
    if s.storage.head(&v.key).await?.is_none() {
        let count = video_parts(v.size);
        let mut parts = s.storage.list_parts(&v.key, &v.upload).await?;
        parts.sort_by_key(|p| p.number);
        let complete = parts.len() as i64 == count
            && parts.iter().enumerate().all(|(i, p)| {
                i64::from(p.number) == i as i64 + 1
                    && p.size == video_part_len(v.size, i as i64 + 1)
            });
        if !complete {
            return Err(Error::PolicyGate("UPLOAD_PARTS_INCOMPLETE"));
        }
        s.storage
            .complete_multipart(&v.key, &v.upload, &parts)
            .await?;
    }
    let meta = s.storage.head(&v.key).await?.ok_or(Error::Conflict)?;
    let head = s.storage.read_prefix(&v.key, 12).await?;
    let inspected = if meta.size == v.size && is_video(&head) {
        crate::video::inspect(s.storage.as_ref(), &v.key, meta.size as u64).await?
    } else {
        None
    };
    let Some(crate::video::Inspection { info, report }) = inspected else {
        let _ = s.storage.delete(&v.key).await;
        sqlx::query("UPDATE portal.addon_files SET status='FAILED' WHERE id=$1")
            .bind(id)
            .execute(&s.pool)
            .await?;
        return Err(Error::PolicyGate("ADDON_VIDEO_INVALID"));
    };
    let positive = |n: i64| (n > 0).then_some(n);
    // Not a gate: staff decide on delivery; the artist sees the issues now.
    let checks = report.as_ref().map(crate::video::checks);
    sqlx::query("UPDATE portal.addon_files SET status='READY', completed_at=now(), duration_ms=$2, width=$3, height=$4, video_report=$5, video_checks=$6 WHERE id=$1 AND status='UPLOADING'")
        .bind(id)
        .bind(positive(info.duration_ms))
        .bind(positive(i64::from(info.width)).map(|n| n as i32))
        .bind(positive(i64::from(info.height)).map(|n| n as i32))
        .bind(&report)
        .bind(&checks)
        .execute(&s.pool)
        .await?;
    Ok(
        json!({"id": id, "status": "READY", "duration": video_clock(info.duration_ms), "width": info.width, "height": info.height, "checks": checks}),
    )
}

/// `m:ss` of a probed length, `""` when the file did not say.
fn video_clock(ms: i64) -> String {
    if ms > 0 {
        crate::mp4::clock(ms)
    } else {
        String::new()
    }
}

/// MP4 and QuickTime files open with a box whose type is `ftyp` (bytes 4–8).
pub fn is_video(head: &[u8]) -> bool {
    head.len() >= 8 && &head[4..8] == b"ftyp"
}

/// A ten-minute download link to the order's video, for staff.
pub async fn staff_video(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT f.object_key, f.name FROM portal.addon_orders o JOIN portal.addon_files f
             ON f.org_id=o.org_id AND f.id::text = o.detail->>'video_file' AND f.status='READY'
          WHERE o.id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.rollback().await?;
    let (key, name) = row.ok_or(Error::NotFound)?;
    let url = s
        .storage
        .presign_get(
            &key,
            &name,
            chrono::Utc::now() + chrono::Duration::minutes(10),
        )
        .await?;
    Ok(json!({"url": url}))
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
        "SELECT {ORDER_JSON} || jsonb_build_object('org_name',g.name) FROM portal.addon_orders o
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
        close_and_refund(&mut tx, &s.config.origin, id, "REJECTED", i.note.trim()).await?;
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
// ---------------------------------------------------------------------------// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_config(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(config(&s, &a, org).await?))
}
async fn h_release(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(release_orders(&s, &a, org, id).await?))
}
async fn h_pay(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PayInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(pay(&s, &a, org, id, i).await?))
}
async fn h_confirm(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<Value>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(confirm(&s, &a, org, id, i).await?))
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
async fn h_video_start(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VideoInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(video_start(&s, &a, org, i).await?))
}
async fn h_video_stored(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(video_stored(&s, &a, org, id).await?))
}
async fn h_video_parts(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<VideoParts>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(video_part_urls(&s, &a, org, id, i).await?))
}
async fn h_video_complete(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(video_complete(&s, &a, org, id).await?))
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
async fn h_staff_video(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_video(&s, &h, id).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/addons/config", get(h_config))
        .route("/api/orgs/{org}/releases/{id}/addons", get(h_release))
        .route("/api/orgs/{org}/releases/{id}/addons/pay", post(h_pay))
        .route(
            "/api/orgs/{org}/addon-payments/{id}/confirm",
            post(h_confirm),
        )
        .route("/api/orgs/{org}/addons/{id}/cancel", post(h_cancel))
        .route("/api/orgs/{org}/addon-videos", post(h_video_start))
        .route(
            "/api/orgs/{org}/addon-videos/{id}/parts",
            get(h_video_stored).post(h_video_parts),
        )
        .route(
            "/api/orgs/{org}/addon-videos/{id}/complete",
            post(h_video_complete),
        )
        .route(
            "/api/payments/payple/return",
            get(h_payple_return).post(h_payple_return),
        )
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
        .route("/api/staff/addons/{id}/video", get(h_staff_video))
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

    fn base_link() -> Value {
        json!({"mvVideoUrl": "https://drive.example.com/mv.mp4", "mvTitle": "여름의 끝 MV", "mvPlatforms": ["melon", "youtube"]})
    }

    #[test]
    fn mv_details_follow_the_tier() {
        let base = base_link();
        assert!(mv_detail("REVIEW", &base, None).is_ok());
        // Distribution only needs the rating it already has.
        assert!(mv_detail("BASIC", &base, None).is_err());
        let mut rated = base.clone();
        rated["mvRating"] = json!("15");
        rated["mvRatingNo"] = json!("제2026-MV0001호");
        assert_eq!(mv_detail("BASIC", &rated, None).unwrap()["rating"], "15");
        let mut bad = base.clone();
        bad["mvPlatforms"] = json!(["netflix"]);
        assert!(mv_detail("REVIEW", &bad, None).is_err());
        let mut http = base.clone();
        http["mvVideoUrl"] = json!("http://drive.example.com/mv.mp4");
        assert!(mv_detail("REVIEW", &http, None).is_err());
        // A file instead of a link.
        let mut file = base;
        file["mvVideoUrl"] = json!("");
        assert!(mv_detail("REVIEW", &file, None).is_err());
        // Length and size come from the file; a typed length is ignored.
        let d = mv_detail(
            "REVIEW",
            &file,
            Some(VideoFile {
                id: Uuid::nil(),
                duration: "3:46".into(),
                width: Some(1920),
                height: Some(1080),
            }),
        )
        .unwrap();
        assert_eq!(d["video_file"], Uuid::nil().to_string());
        assert_eq!(
            (d["duration"].as_str(), d["width"].as_i64()),
            (Some("3:46"), Some(1920))
        );
        // A link is measured by staff.
        assert_eq!(
            mv_detail("REVIEW", &base_link(), None).unwrap()["duration"],
            ""
        );
    }

    #[test]
    fn videos_are_mp4_or_quicktime() {
        assert!(is_video(b"\0\0\0\x20ftypisom"));
        assert!(is_video(b"\0\0\0\x14ftypqt  "));
        assert!(!is_video(b"RIFF....WAVE"));
        assert!(!is_video(b"ftyp"));
    }
}
