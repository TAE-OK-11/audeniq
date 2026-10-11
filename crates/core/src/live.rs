//! Live updates (Server-Sent Events) for the studio and the admin console.
//!
//! The API runs behind PgBouncer in transaction mode, so a connection cannot
//! `LISTEN`. Instead each open stream checks a few cheap "version" values
//! (latest notification, latest inquiry change, latest work change) every
//! [`TICK`] and sends a small event only when one moved. The client then
//! re-fetches the affected list through the normal, authorized endpoints:
//! the stream itself carries no customer content.
//!
//! - `GET /api/orgs/{org}/live` — a studio member: `notifications`,
//!   `inquiries` (with the changed inquiry and whether staff wrote last).
//! - `GET /api/staff/live` — staff: `inquiries` (with the last message's
//!   author kind, for the new-message alert) and `work`.
//!
//! A stream lives at most [`MAX_LIFETIME`] and re-checks the session (and
//! the membership / staff role) every [`RECHECK`]; EventSource reconnects on
//! its own, so revoked access ends within a minute. The edge Worker passes
//! the body through untouched and the compression layer skips
//! `text/event-stream`.
use crate::{api::AppState, auth, error::Result, staff};
use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::Row;
use std::{convert::Infallible, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

const TICK: Duration = Duration::from_secs(3);
const RECHECK: Duration = Duration::from_secs(60);
const MAX_LIFETIME: Duration = Duration::from_secs(600);
/// Stream opens per user per 15 minutes (reconnects every 10 minutes, tabs, reloads).
const OPENS_PER_WINDOW: i32 = 120;

type LiveStream = Response;

fn event(name: &str, data: Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

fn stream(rx: mpsc::Receiver<std::result::Result<Event, Infallible>>) -> LiveStream {
    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
        .into_response()
}

#[derive(Debug, Clone, PartialEq)]
struct OrgVersion {
    notification: Option<DateTime<Utc>>,
    unread: i64,
    inquiry: Option<DateTime<Utc>>,
}

async fn org_version(
    s: &AppState,
    org: Uuid,
    user: Uuid,
    party: Uuid,
) -> Result<(OrgVersion, Value)> {
    let r = sqlx::query(
        "SELECT
           (SELECT max(n.created_at) FROM portal.notifications n WHERE n.org_id=$1 AND (n.user_id IS NULL OR n.user_id=$2)) AS notification,
           (SELECT count(*) FROM portal.notifications n WHERE n.org_id=$1 AND (n.user_id IS NULL OR n.user_id=$2)
              AND NOT EXISTS(SELECT 1 FROM portal.notification_reads r WHERE r.notification_id=n.id AND r.user_id=$2)) AS unread,
           q.updated_at AS inquiry, q.id AS inquiry_id,
           (SELECT m.author_kind FROM portal.inquiry_messages m WHERE m.inquiry_id=q.id ORDER BY m.created_at DESC LIMIT 1) AS author
         FROM (SELECT 1) one
         LEFT JOIN LATERAL (SELECT i.id, i.updated_at FROM portal.inquiries i WHERE i.org_id=$1
             -- Same visibility as list_inquiries: release-linked inquiries
             -- only for members who can read that release.
             AND (i.release_id IS NULL OR EXISTS(
               SELECT 1 FROM identity.resource_acl acl WHERE acl.org_id=i.org_id AND acl.resource_id=i.release_id
                 AND acl.principal_party_id=$3 AND acl.action='read' AND acl.revoked_at IS NULL
                 AND acl.starts_at<=now() AND (acl.ends_at IS NULL OR acl.ends_at>now())))
             ORDER BY i.updated_at DESC LIMIT 1) q ON true",
    )
    .bind(org)
    .bind(user)
    .bind(party)
    .fetch_one(&s.pool)
    .await?;
    let v = OrgVersion {
        notification: r.get("notification"),
        unread: r.get("unread"),
        inquiry: r.get("inquiry"),
    };
    let last = json!({"inquiry_id": r.get::<Option<Uuid>,_>("inquiry_id"), "staff": r.get::<Option<String>,_>("author").as_deref() == Some("STAFF")});
    Ok((v, last))
}

/// Studio member stream for one workspace.
pub async fn org_live(s: AppState, h: HeaderMap, org: Uuid) -> Result<LiveStream> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    {
        let mut c = s.pool.acquire().await?;
        auth::membership(&mut c, &a, org, false).await?;
    }
    auth::rate(&s.pool, &format!("live:{}", a.user), OPENS_PER_WINDOW).await?;
    let (tx, rx) = mpsc::channel(8);
    let user = a.user;
    let party = a.party;
    tokio::spawn(async move {
        let started = tokio::time::Instant::now();
        let mut checked = started;
        let Ok((mut seen, _)) = org_version(&s, org, user, party).await else {
            return;
        };
        if tx
            .send(Ok(event("ready", json!({"unread": seen.unread}))))
            .await
            .is_err()
        {
            return;
        }
        loop {
            tokio::time::sleep(TICK).await;
            if tx.is_closed() || started.elapsed() > MAX_LIFETIME {
                return;
            }
            if checked.elapsed() > RECHECK {
                checked = tokio::time::Instant::now();
                let still = async {
                    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
                    let mut c = s.pool.acquire().await?;
                    auth::membership(&mut c, &a, org, false).await
                }
                .await;
                if still.is_err() {
                    let _ = tx
                        .send(Ok(event("end", json!({"reason": "session"}))))
                        .await;
                    return;
                }
            }
            let Ok((now, last)) = org_version(&s, org, user, party).await else {
                continue;
            };
            let changed = now.notification != seen.notification || now.unread != seen.unread;
            let fresh = json!({"unread": now.unread, "new": now.notification > seen.notification});
            if changed && tx.send(Ok(event("notifications", fresh))).await.is_err() {
                return;
            }
            if now.inquiry != seen.inquiry && tx.send(Ok(event("inquiries", last))).await.is_err() {
                return;
            }
            seen = now;
        }
    });
    Ok(stream(rx))
}

#[derive(Debug, Clone, PartialEq)]
struct StaffVersion {
    inquiry: Option<DateTime<Utc>>,
    open: i64,
    work: Option<DateTime<Utc>>,
}

async fn staff_version(s: &AppState) -> Result<(StaffVersion, Value)> {
    let r = sqlx::query(
        "SELECT
           (SELECT max(updated_at) FROM portal.inquiries) AS inquiry,
           (SELECT count(*) FROM portal.inquiries WHERE status='OPEN') AS open,
           GREATEST((SELECT max(updated_at) FROM work.tasks),
                    (SELECT max(GREATEST(created_at, edited_at, resolved_at, archived_at)) FROM work.notes)) AS work,
           m.inquiry_id, m.author_kind, q.subject, o.name AS org
         FROM (SELECT 1) one
         LEFT JOIN LATERAL (SELECT inquiry_id, author_kind FROM portal.inquiry_messages ORDER BY created_at DESC LIMIT 1) m ON true
         LEFT JOIN portal.inquiries q ON q.id=m.inquiry_id
         LEFT JOIN identity.orgs o ON o.id=q.org_id",
    )
    .fetch_one(&s.pool)
    .await?;
    let v = StaffVersion {
        inquiry: r.get("inquiry"),
        open: r.get("open"),
        work: r.get("work"),
    };
    let last = json!({
        "open": v.open,
        "inquiry_id": r.get::<Option<Uuid>,_>("inquiry_id"),
        "customer": r.get::<Option<String>,_>("author_kind").is_some_and(|k| k != "STAFF"),
        "subject": r.get::<Option<String>,_>("subject"),
        "org": r.get::<Option<String>,_>("org"),
    });
    Ok((v, last))
}

/// Staff stream: customer inquiries and internal work changes.
pub async fn staff_live(s: AppState, h: HeaderMap) -> Result<LiveStream> {
    let st = staff::staff(&s, &h, false).await?;
    auth::rate(
        &s.pool,
        &format!("live:{}", st.actor.user),
        OPENS_PER_WINDOW,
    )
    .await?;
    let (tx, rx) = mpsc::channel(8);
    tokio::spawn(async move {
        let started = tokio::time::Instant::now();
        let mut checked = started;
        let Ok((mut seen, _)) = staff_version(&s).await else {
            return;
        };
        if tx
            .send(Ok(event("ready", json!({"open": seen.open}))))
            .await
            .is_err()
        {
            return;
        }
        loop {
            tokio::time::sleep(TICK).await;
            if tx.is_closed() || started.elapsed() > MAX_LIFETIME {
                return;
            }
            if checked.elapsed() > RECHECK {
                checked = tokio::time::Instant::now();
                if staff::staff(&s, &h, false).await.is_err() {
                    let _ = tx
                        .send(Ok(event("end", json!({"reason": "session"}))))
                        .await;
                    return;
                }
            }
            let Ok((now, last)) = staff_version(&s).await else {
                continue;
            };
            if (now.inquiry != seen.inquiry || now.open != seen.open)
                && tx.send(Ok(event("inquiries", last))).await.is_err()
            {
                return;
            }
            if now.work != seen.work && tx.send(Ok(event("work", json!({})))).await.is_err() {
                return;
            }
            seen = now;
        }
    });
    Ok(stream(rx))
}

async fn h_org_live(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<LiveStream> {
    org_live(s, h, org).await
}
async fn h_staff_live(State(s): State<AppState>, h: HeaderMap) -> Result<LiveStream> {
    staff_live(s, h).await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/live", get(h_org_live))
        .route("/api/staff/live", get(h_staff_live))
}
