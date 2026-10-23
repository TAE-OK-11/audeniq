//! Release alerts (migration 0118).
//!
//! Before the release date a fan can leave an email on the smart link and
//! gets exactly one mail on release day (from 09:00 KST) with the page link.
//! The address is sealed at once and erased when the mail is sent, the alert
//! is cancelled (page gone) or it expires (not sent within a week of the
//! release date). A keyed blind index only stops duplicate sign-ups; the
//! answer never tells whether an address was already on the list.
use crate::{
    api::AppState,
    auth,
    email::{Mailer, Rendered, escape_html, one_line},
    error::{Error, Result},
    payout_keys::{Field, KeyRing},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Alerts sent per round.
pub const BATCH: i64 = 20;
const MAX_ATTEMPTS: i32 = 6;
/// Not sent this many days after the release date → expired.
const EXPIRE_DAYS: i32 = 7;
const LEASE_MINUTES: i32 = 10;

fn normalize_email(v: &str) -> Result<String> {
    let v = v.trim().to_lowercase();
    let ok = v.len() <= 254
        && !v.chars().any(|c| c.is_whitespace() || c.is_control())
        && v.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
        });
    if ok {
        Ok(v)
    } else {
        Err(Error::InvalidCode("ALERT_EMAIL_INVALID"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignUp {
    pub email: String,
    pub consent: bool,
}

/// A fan asks to be told when the release is out.
pub async fn sign_up(s: &AppState, h: &HeaderMap, slug: &str, i: SignUp) -> Result<Value> {
    if !i.consent {
        return Err(Error::InvalidCode("ALERT_CONSENT_REQUIRED"));
    }
    let mail = normalize_email(&i.email)?;
    let row = crate::smart_links::visible(s, slug).await?;
    let date: Option<String> = row.get("release_date");
    // Open until the release is out (its simultaneous release time included).
    if date.is_none()
        || crate::smart_links::is_released(
            date.as_deref(),
            row.get::<Option<String>, _>("release_time").as_deref(),
        )
    {
        return Err(Error::PolicyGate("ALERT_ALREADY_RELEASED"));
    }
    let link: Uuid = row.get("id");
    let ip = auth::client_ip_key(h).unwrap_or_else(|| "unknown".into());
    auth::rate(&s.pool, &format!("release-alert:{ip}"), 10).await?;
    auth::rate(&s.pool, &format!("release-alert-mail:{mail}"), 5).await?;
    auth::rate(&s.pool, "release-alert:all", 3000).await?;
    let keys = &s.payout_keys;
    let known: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.release_alerts WHERE link_id=$1 AND email_bidx = ANY($2))",
    )
    .bind(link)
    .bind(keys.blind_indexes(Field::ReleaseAlertEmail, &mail))
    .fetch_one(&s.pool)
    .await?;
    if !known {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO portal.release_alerts(id, link_id, email_cipher, email_bidx) VALUES($1,$2,$3,$4)
             ON CONFLICT (link_id, email_bidx) DO NOTHING",
        )
        .bind(id)
        .bind(link)
        .bind(keys.seal_field(Field::ReleaseAlertEmail, id, &mail)?)
        .bind(keys.blind_index(Field::ReleaseAlertEmail, &mail)?)
        .execute(&s.pool)
        .await?;
    }
    Ok(json!({"ok": true}))
}

/// The one mail a fan gets.
pub fn render(origin: &str, slug: &str, title: &str, artist: Option<&str>) -> Rendered {
    let title = one_line(title);
    let artist = artist.map(one_line).filter(|a| !a.is_empty());
    let url = format!("{origin}/l/{slug}");
    let who = artist
        .as_deref()
        .map(|a| format!("{a}의 "))
        .unwrap_or_default();
    let headline = format!("{who}'{title}'이(가) 발매됐어요");
    let footer = "발매 알림을 신청하셔서 한 번만 보내 드리는 메일이에요. 보낸 뒤 이메일 주소는 바로 지웠어요.";
    let text = format!("{headline}\n\n지금 들어 보세요: {url}\n\n--\n{footer}\n");
    let html = format!(
        "<!doctype html><html lang=\"ko\"><body style=\"margin:0;padding:24px;background:#f5f6f8;font-family:'Pretendard Variable',Pretendard,-apple-system,'Apple SD Gothic Neo','Malgun Gothic',sans-serif\">\
<div style=\"max-width:520px;margin:0 auto;background:#ffffff;border-radius:16px;padding:28px\">\
<h1 style=\"margin:0 0 18px;font-size:20px;line-height:1.4;color:#111827\">{headline}</h1>\
<a href=\"{url}\" style=\"display:inline-block;padding:12px 20px;border-radius:10px;background:#3b63f6;color:#ffffff;text-decoration:none;font-weight:700\">지금 듣기</a>\
<p style=\"margin:24px 0 0;font-size:12px;line-height:1.6;color:#6b7280\">{footer}</p>\
</div></body></html>",
        headline = escape_html(&headline),
        url = escape_html(&url),
    );
    Rendered {
        subject: headline,
        text,
        html,
    }
}

async fn finish(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    provider: Option<&str>,
    code: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE portal.release_alerts SET status=$2, provider=$3, last_error=$4, email_cipher=NULL,
            sent_at=CASE WHEN $2='SENT' THEN now() ELSE sent_at END
          WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(provider)
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

fn backoff_seconds(attempts: i32) -> f64 {
    match attempts {
        ..=1 => 60.0,
        2 => 300.0,
        3 => 1800.0,
        4 => 7200.0,
        _ => 21600.0,
    }
}

/// Send the alerts that are due: release date reached (from 09:00 KST).
pub async fn dispatch_once(pool: &PgPool, keys: &KeyRing, mailer: &Mailer) -> Result<usize> {
    // Due from the release instant (date + simultaneous release time), and
    // never before 09:00 KST on the day.
    let now_kst = (chrono::Utc::now() + chrono::Duration::hours(9))
        .format("%Y-%m-%d %H:%M")
        .to_string();
    let rows = sqlx::query(
        "UPDATE portal.release_alerts a
            SET attempts=a.attempts+1, next_attempt_at=now() + make_interval(mins => $2)
          WHERE a.id IN (
            SELECT a2.id FROM portal.release_alerts a2
              JOIN portal.smart_links l ON l.id=a2.link_id
              JOIN catalog.releases r ON r.id=l.release_id
             WHERE a2.status='PENDING' AND a2.next_attempt_at <= now()
               AND (r.draft->>'release_date') || ' ' ||
                   greatest(COALESCE(NULLIF(r.draft->>'release_time', ''), '00:00'), '09:00') <= $3
             ORDER BY a2.next_attempt_at LIMIT $1 FOR UPDATE OF a2 SKIP LOCKED)
          RETURNING a.id, a.link_id, a.attempts, a.email_cipher",
    )
    .bind(BATCH)
    .bind(LEASE_MINUTES)
    .bind(&now_kst)
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("attempts");
        let page = match crate::smart_links::visible_by_id(pool, row.get("link_id")).await {
            Ok(p) => p,
            Err(Error::NotFound) => {
                finish(pool, id, "CANCELLED", None, Some("ALERT_PAGE_GONE")).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        let sealed: Option<Vec<u8>> = row.get("email_cipher");
        let to = match sealed.map(|c| keys.open_field(Field::ReleaseAlertEmail, id, &c)) {
            Some(Ok(to)) => to,
            _ => {
                finish(pool, id, "FAILED", None, Some("EMAIL_ADDRESS_UNAVAILABLE")).await?;
                continue;
            }
        };
        let mail = render(
            &mailer.origin,
            &page.get::<String, _>("slug"),
            &page.get::<String, _>("title"),
            page.get::<Option<String>, _>("artist").as_deref(),
        );
        match mailer.send(id, &to, &mail).await {
            Ok(provider) => finish(pool, id, "SENT", Some(provider), None).await?,
            Err(Error::PolicyGate("EMAIL_ADDRESS_INVALID")) => {
                finish(pool, id, "FAILED", None, Some("EMAIL_ADDRESS_INVALID")).await?
            }
            Err(_) if attempts >= MAX_ATTEMPTS => {
                finish(pool, id, "FAILED", None, Some("EMAIL_SEND_FAILED")).await?
            }
            Err(_) => {
                sqlx::query(
                    "UPDATE portal.release_alerts SET last_error='EMAIL_SEND_FAILED',
                        next_attempt_at=now() + make_interval(secs => $2) WHERE id=$1",
                )
                .bind(id)
                .bind(backoff_seconds(attempts))
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(rows.len())
}

/// Erase addresses that can no longer be sent: a week past the release date,
/// or the release date was removed. Runs even without a mail provider.
pub async fn sweep(pool: &PgPool) -> Result<()> {
    let cutoff = (chrono::Utc::now() + chrono::Duration::hours(9)
        - chrono::Duration::days(EXPIRE_DAYS.into()))
    .date_naive()
    .to_string();
    sqlx::query(
        "UPDATE portal.release_alerts a SET status='EXPIRED', last_error='ALERT_EXPIRED', email_cipher=NULL
           FROM portal.smart_links l JOIN catalog.releases r ON r.id=l.release_id
          WHERE a.link_id=l.id AND a.status='PENDING'
            AND (r.draft->>'release_date' IS NULL OR r.draft->>'release_date' < $1)",
    )
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(())
}

async fn h_sign_up(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    h: HeaderMap,
    Json(i): Json<SignUp>,
) -> Result<Json<Value>> {
    Ok(Json(sign_up(&s, &h, &slug, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/smart-links/{slug}/alerts", post(h_sign_up))
}
