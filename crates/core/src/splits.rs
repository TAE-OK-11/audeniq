//! Revenue splits with collaborators (migration 0119).
//!
//! While a release can still be edited (from the application on), the
//! applicant names, per track, the collaborators who share that track's net
//! royalty (after the AUDENIQ fee) and their percentages; the applicant keeps
//! the rest. Each collaborator gets an email invite and accepts into their own
//! AUDENIQ account (as its OWNER), so their share is credited to their own
//! balance and paid and taxed under their own details. Until they answer,
//! their share of posted royalties is held; accepting moves it to them,
//! declining (or being removed) returns it to the applicant
//! ([`crate::royalty_posting`], `finance.settle_split_share`).
//!
//! Invite addresses are sealed and erased once the invite is answered or the
//! collaborator is removed. Changing an accepted percentage asks again.
use crate::{
    api::AppState,
    auth::{self, Actor},
    email::{Mailer, Rendered, escape_html, one_line},
    error::{Error, Result},
    operations,
    payout_keys::{Field, KeyRing},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

/// Collaborators per track.
const MAX_SHARES: usize = 10;
/// Invites sent per round.
pub const BATCH: i64 = 20;
const MAX_ATTEMPTS: i32 = 6;
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
        Err(Error::InvalidCode("SPLIT_EMAIL_INVALID"))
    }
}

/// "ab***@example.com"
fn mask(email: &str) -> String {
    match email.split_once('@') {
        Some((l, d)) => {
            let head: String = l.chars().take(2).collect();
            format!("{head}***@{d}")
        }
        None => "***".into(),
    }
}

fn pct(bps: i32) -> String {
    if bps % 100 == 0 {
        format!("{}%", bps / 100)
    } else {
        let frac = format!("{:02}", bps % 100);
        format!("{}.{}%", bps / 100, frac.trim_end_matches('0'))
    }
}

/// A placeholder token hash: the real token is made when the invite is sent.
fn unguessable_hash() -> String {
    crate::domain::sha256_hex(auth::random_token())
}

async fn editable(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<bool> {
    let ok: Option<bool> = sqlx::query_scalar(
        "SELECT catalog.is_editable_status(status) FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?;
    ok.ok_or(Error::NotFound)
}

// ---------------------------------------------------------------------------
// Applicant
// ---------------------------------------------------------------------------

/// The split sheet of a release: per track, the collaborators and what each
/// share has received or holds.
pub async fn sheet(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let can_edit = editable(&mut tx, org, release).await?;
    let tracks = sqlx::query(
        "SELECT id, title, disc_number, track_number FROM catalog.tracks
          WHERE org_id=$1 AND release_id=$2 AND archived_at IS NULL ORDER BY disc_number, track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let shares = sqlx::query(
        "SELECT sh.id, sh.track_id, sh.name, sh.email_cipher, sh.share_bps, sh.status, sh.invite_status,
                o.name AS recipient_name,
                COALESCE((SELECT SUM(amount) FROM finance.split_allocations x WHERE x.share_id=sh.id AND x.status='HELD'),0)::text AS held,
                COALESCE((SELECT SUM(amount) FROM finance.split_allocations x WHERE x.share_id=sh.id AND x.status='TRANSFERRED'),0)::text AS transferred
           FROM portal.split_shares sh LEFT JOIN identity.orgs o ON o.id=sh.recipient_org_id
          WHERE sh.org_id=$1 AND sh.release_id=$2 AND sh.status <> 'REMOVED'
          ORDER BY sh.created_at",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    let mut by_track: BTreeMap<Uuid, Vec<Value>> = BTreeMap::new();
    for r in shares {
        let id: Uuid = r.get("id");
        let email = r
            .get::<Option<Vec<u8>>, _>("email_cipher")
            .and_then(|c| s.payout_keys.open_field(Field::SplitEmail, id, &c).ok())
            .map(|e| mask(&e));
        by_track.entry(r.get("track_id")).or_default().push(json!({
            "id": id, "name": r.get::<String, _>("name"), "email": email,
            "share_bps": r.get::<i32, _>("share_bps"), "status": r.get::<String, _>("status"),
            "invite_status": r.get::<String, _>("invite_status"),
            "recipient_name": r.get::<Option<String>, _>("recipient_name"),
            "held": r.get::<String, _>("held"), "transferred": r.get::<String, _>("transferred"),
        }));
    }
    let tracks: Vec<Value> = tracks
        .into_iter()
        .map(|t| {
            let id: Uuid = t.get("id");
            let shares = by_track.remove(&id).unwrap_or_default();
            let given: i64 = shares
                .iter()
                .filter(|v| v["status"] != "DECLINED")
                .filter_map(|v| v["share_bps"].as_i64())
                .sum();
            json!({"track_id": id, "title": t.get::<String, _>("title"), "disc": t.get::<i32, _>("disc_number"),
                   "number": t.get::<i32, _>("track_number"), "shares": shares, "owner_bps": 10_000 - given})
        })
        .collect();
    Ok(json!({"editable": can_edit, "tracks": tracks}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareInput {
    /// An existing collaborator of this track (the screen only sees their
    /// address masked); new collaborators come without one.
    #[serde(default)]
    pub id: Option<Uuid>,
    pub name: String,
    /// Required for a new collaborator, and to ask again someone who already
    /// answered (their address was erased).
    #[serde(default)]
    pub email: Option<String>,
    /// Basis points: 2550 = 25.5%.
    pub share_bps: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackInput {
    pub track_id: Uuid,
    #[serde(default)]
    pub shares: Vec<ShareInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetInput {
    pub tracks: Vec<TrackInput>,
}

struct Planned {
    id: Option<Uuid>,
    name: String,
    /// (address, active index, every index across key versions)
    email: Option<(String, String, Vec<String>)>,
    bps: i32,
}

/// Replace the collaborators of the listed tracks. Kept people keep their
/// answer unless their percentage changes; dropped ones are removed (their
/// address erased, anything held for them returned); new ones are invited.
pub async fn save(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: SheetInput,
) -> Result<Value> {
    let keys = &s.payout_keys;
    // Validate everything before touching the database.
    let mut plan: Vec<(Uuid, Vec<Planned>)> = Vec::new();
    let mut seen_tracks = HashSet::new();
    for t in &i.tracks {
        if !seen_tracks.insert(t.track_id) || t.shares.len() > MAX_SHARES {
            return Err(Error::InvalidCode("SPLIT_INVALID"));
        }
        let mut total = 0i32;
        let mut seen_mail = HashSet::new();
        let mut seen_ids = HashSet::new();
        let mut rows = Vec::new();
        for sh in &t.shares {
            let name = sh.name.trim().to_string();
            if name.is_empty() || name.chars().count() > 100 {
                return Err(Error::InvalidCode("SPLIT_NAME_INVALID"));
            }
            crate::text_policy::check(&name)?;
            if !(1..=10_000).contains(&sh.share_bps) {
                return Err(Error::InvalidCode("SPLIT_SHARE_INVALID"));
            }
            total += sh.share_bps;
            if let Some(id) = sh.id
                && !seen_ids.insert(id)
            {
                return Err(Error::InvalidCode("SPLIT_INVALID"));
            }
            let email = match sh.email.as_deref().filter(|e| !e.trim().is_empty()) {
                Some(e) => {
                    let e = normalize_email(e)?;
                    if !seen_mail.insert(e.clone()) {
                        return Err(Error::InvalidCode("SPLIT_DUPLICATE_EMAIL"));
                    }
                    let bidx = keys.blind_index(Field::SplitEmail, &e)?;
                    let all = keys.blind_indexes(Field::SplitEmail, &e);
                    Some((e, bidx, all))
                }
                None if sh.id.is_none() => return Err(Error::InvalidCode("SPLIT_EMAIL_INVALID")),
                None => None,
            };
            rows.push(Planned {
                id: sh.id,
                name,
                email,
                bps: sh.share_bps,
            });
        }
        if total > 10_000 {
            return Err(Error::InvalidCode("SPLIT_TOTAL_OVER_100"));
        }
        plan.push((t.track_id, rows));
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    if !editable(&mut tx, org, release).await? {
        return Err(Error::PolicyGate("SPLIT_LOCKED"));
    }
    // One save of a release's sheet at a time: two tabs adding different
    // people must not together pass 100% on a track.
    sqlx::query("SELECT 1 FROM catalog.releases WHERE org_id=$1 AND id=$2 FOR UPDATE")
        .bind(org)
        .bind(release)
        .execute(&mut *tx)
        .await?;
    let mut invited = 0;
    for (track, rows) in plan {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND id=$3 AND archived_at IS NULL)",
        )
        .bind(org)
        .bind(release)
        .bind(track)
        .fetch_one(&mut *tx)
        .await?;
        if !ok {
            return Err(Error::InvalidCode("SPLIT_INVALID"));
        }
        let existing = sqlx::query(
            "SELECT id, email_bidx, share_bps, status, email_cipher IS NOT NULL AS has_email FROM portal.split_shares
              WHERE track_id=$1 AND org_id=$2 AND status <> 'REMOVED' FOR UPDATE",
        )
        .bind(track)
        .bind(org)
        .fetch_all(&mut *tx)
        .await?;
        // Someone new must not be someone already listed.
        for r in rows.iter().filter(|r| r.id.is_none()) {
            let all = &r.email.as_ref().expect("checked").2;
            if existing
                .iter()
                .any(|e| all.contains(&e.get::<String, _>("email_bidx")))
            {
                return Err(Error::InvalidCode("SPLIT_DUPLICATE_EMAIL"));
            }
        }
        let kept: HashSet<Uuid> = rows.iter().filter_map(|r| r.id).collect();
        if kept
            .iter()
            .any(|id| !existing.iter().any(|e| e.get::<Uuid, _>("id") == *id))
        {
            return Err(Error::InvalidCode("SPLIT_INVALID"));
        }
        for e in &existing {
            let id: Uuid = e.get("id");
            if !kept.contains(&id) {
                sqlx::query(
                    "UPDATE portal.split_shares SET status='REMOVED', recipient_org_id=NULL, email_cipher=NULL,
                            invite_status=CASE WHEN invite_status='PENDING' THEN 'SKIPPED' ELSE invite_status END, decided_at=now()
                      WHERE id=$1",
                )
                .bind(id)
                .execute(&mut *tx)
                .await?;
                sqlx::query("SELECT finance.settle_split_share($1)")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        for r in rows {
            match r.id {
                Some(id) => {
                    let e = existing
                        .iter()
                        .find(|e| e.get::<Uuid, _>("id") == id)
                        .expect("checked");
                    // A declined share left as it was stays declined (asking again needs an address).
                    let unchanged = e.get::<i32, _>("share_bps") == r.bps && r.email.is_none();
                    if unchanged {
                        sqlx::query("UPDATE portal.split_shares SET name=$2 WHERE id=$1")
                            .bind(id)
                            .bind(&r.name)
                            .execute(&mut *tx)
                            .await?;
                        continue;
                    }
                    // A new percentage (or asking again) needs a new answer, to the
                    // address on file while the invite is open, else a new one.
                    let cipher = match &r.email {
                        Some((mail, bidx, all)) => {
                            if !all.contains(&e.get::<String, _>("email_bidx"))
                                && existing.iter().any(|x| {
                                    x.get::<Uuid, _>("id") != id
                                        && all.contains(&x.get::<String, _>("email_bidx"))
                                })
                            {
                                return Err(Error::InvalidCode("SPLIT_DUPLICATE_EMAIL"));
                            }
                            Some((keys.seal_field(Field::SplitEmail, id, mail)?, bidx.clone()))
                        }
                        None if e.get::<bool, _>("has_email") => None,
                        None => return Err(Error::InvalidCode("SPLIT_EMAIL_REQUIRED")),
                    };
                    // Anything already credited stays theirs; later postings wait for the new answer.
                    sqlx::query(
                        "UPDATE portal.split_shares SET name=$2, share_bps=$3, status='INVITED', recipient_org_id=NULL,
                                email_cipher=COALESCE($4, email_cipher), email_bidx=COALESCE($5, email_bidx),
                                invite_token_hash=$6, invite_status='PENDING', invite_attempts=0, invite_next_at=now(), decided_at=NULL
                          WHERE id=$1",
                    )
                    .bind(id)
                    .bind(&r.name)
                    .bind(r.bps)
                    .bind(cipher.as_ref().map(|c| c.0.clone()))
                    .bind(cipher.as_ref().map(|c| c.1.clone()))
                    .bind(unguessable_hash())
                    .execute(&mut *tx)
                    .await?;
                    invited += 1;
                }
                None => {
                    let (mail, bidx, _) = r.email.expect("checked");
                    let id = Uuid::new_v4();
                    sqlx::query(
                        "INSERT INTO portal.split_shares(id, org_id, release_id, track_id, name, email_cipher, email_bidx, share_bps,
                                                         invite_token_hash, created_by)
                         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                    )
                    .bind(id)
                    .bind(org)
                    .bind(release)
                    .bind(track)
                    .bind(&r.name)
                    .bind(keys.seal_field(Field::SplitEmail, id, &mail)?)
                    .bind(&bidx)
                    .bind(r.bps)
                    .bind(unguessable_hash())
                    .bind(a.user)
                    .execute(&mut *tx)
                    .await?;
                    invited += 1;
                }
            }
        }
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.splits_saved",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"saved": true, "invited": invited}))
}

/// Ask again (a lost email): a new link replaces the old one.
pub async fn resend(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    share: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    auth::rate(&s.pool, &format!("split-resend:{share}"), 3).await?;
    let n = sqlx::query(
        "UPDATE portal.split_shares SET invite_status='PENDING', invite_attempts=0, invite_next_at=now()
          WHERE id=$1 AND org_id=$2 AND release_id=$3 AND status='INVITED' AND email_cipher IS NOT NULL",
    )
    .bind(share)
    .bind(org)
    .bind(release)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    tx.commit().await?;
    Ok(json!({"queued": true}))
}

// ---------------------------------------------------------------------------
// Collaborator
// ---------------------------------------------------------------------------

async fn by_token(c: &mut PgConnection, token: &str) -> Result<sqlx::postgres::PgRow> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::NotFound);
    }
    sqlx::query(
        "SELECT sh.id, sh.org_id, sh.release_id, sh.share_bps, sh.status, sh.name, sh.recipient_org_id,
                r.title, r.draft->>'artist' AS artist, t.title AS track_title, o.name AS org_name
           FROM portal.split_shares sh
           JOIN catalog.releases r ON r.id=sh.release_id
           JOIN catalog.tracks t ON t.id=sh.track_id
           JOIN identity.orgs o ON o.id=sh.org_id
          WHERE sh.invite_token_hash=$1 AND sh.status <> 'REMOVED' FOR UPDATE OF sh",
    )
    .bind(crate::domain::sha256_hex(token.to_ascii_lowercase()))
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

/// What the invite is about (the link is the secret).
pub async fn invite(s: &AppState, token: &str) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let r = by_token(&mut tx, token).await?;
    tx.rollback().await?;
    Ok(json!({
        "title": r.get::<String, _>("title"), "artist": r.get::<Option<String>, _>("artist"),
        "track_title": r.get::<String, _>("track_title"), "from": r.get::<String, _>("org_name"),
        "name": r.get::<String, _>("name"), "share_bps": r.get::<i32, _>("share_bps"),
        "status": r.get::<String, _>("status"),
    }))
}

/// Accept into `org` (the collaborator's own account, as its OWNER).
pub async fn accept(s: &AppState, a: &Actor, org: Uuid, token: &str) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    if auth::membership(&mut tx, a, org, true).await? != "OWNER" {
        return Err(Error::PolicyGate("SPLIT_OWNER_REQUIRED"));
    }
    let r = by_token(&mut tx, token).await?;
    if r.get::<Uuid, _>("org_id") == org {
        return Err(Error::PolicyGate("SPLIT_SELF"));
    }
    if r.get::<String, _>("status") != "INVITED" {
        return Err(Error::PolicyGate("SPLIT_ALREADY_ANSWERED"));
    }
    let id: Uuid = r.get("id");
    sqlx::query(
        "UPDATE portal.split_shares SET status='ACCEPTED', recipient_org_id=$2, email_cipher=NULL, decided_at=now(),
                invite_token_hash=$3, invite_status=CASE WHEN invite_status='PENDING' THEN 'SKIPPED' ELSE invite_status END
          WHERE id=$1",
    )
    .bind(id)
    .bind(org)
    .bind(unguessable_hash())
    .execute(&mut *tx)
    .await?;
    let moved: i32 = sqlx::query_scalar("SELECT finance.settle_split_share($1)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(r.get("release_id")),
        "split.accepted",
        "COLLABORATOR",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"status": "ACCEPTED", "moved": moved}))
}

/// Decline (no account needed: the link is the secret).
pub async fn decline(s: &AppState, token: &str) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let r = by_token(&mut tx, token).await?;
    if r.get::<String, _>("status") != "INVITED" {
        return Err(Error::PolicyGate("SPLIT_ALREADY_ANSWERED"));
    }
    let id: Uuid = r.get("id");
    sqlx::query(
        "UPDATE portal.split_shares SET status='DECLINED', email_cipher=NULL, decided_at=now(), invite_token_hash=$2,
                invite_status=CASE WHEN invite_status='PENDING' THEN 'SKIPPED' ELSE invite_status END
          WHERE id=$1",
    )
    .bind(id)
    .bind(unguessable_hash())
    .execute(&mut *tx)
    .await?;
    sqlx::query("SELECT finance.settle_split_share($1)")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(json!({"status": "DECLINED"}))
}

/// Shares this account receives from other people's releases.
pub async fn received(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',sh.id,'title',r.title,'artist',r.draft->>'artist','track_title',t.title,
                'from',o.name,'share_bps',sh.share_bps,'accepted_at',sh.decided_at,
                'received',COALESCE((SELECT SUM(amount) FROM finance.split_allocations x
                                      WHERE x.share_id=sh.id AND x.status='TRANSFERRED'),0)::text)
           FROM portal.split_shares sh
           JOIN catalog.releases r ON r.id=sh.release_id
           JOIN catalog.tracks t ON t.id=sh.track_id
           JOIN identity.orgs o ON o.id=sh.org_id
          WHERE sh.recipient_org_id=$1 AND sh.status='ACCEPTED'
          ORDER BY sh.decided_at DESC LIMIT 500",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

// ---------------------------------------------------------------------------
// Invite mail
// ---------------------------------------------------------------------------

pub fn render(
    origin: &str,
    token: &str,
    from: &str,
    title: &str,
    track: &str,
    share_bps: i32,
) -> Rendered {
    let from = one_line(from);
    let title = one_line(title);
    let track = one_line(track);
    let url = format!("{origin}/splits/{token}");
    let share = pct(share_bps);
    let headline = format!("{from}에서 '{track}' 수익의 {share}를 나눠 드리려고 해요");
    let body = format!(
        "'{title}'에 실린 '{track}'의 정산 수익(AUDENIQ 수수료 공제 후) 중 {share}가 공동작업 몫으로 정해졌어요. \
         수락하면 이 곡 수익이 들어올 때마다 내 AUDENIQ 계정 잔액에 쌓이고, 내 계좌로 출금할 수 있어요. \
         계정이 없으면 가입한 뒤 수락하면 돼요. 수락 전에 들어온 몫은 보관했다가 수락할 때 함께 옮겨 드려요."
    );
    let footer = "이 링크는 받으신 분만 쓸 수 있어요. 모르는 내용이면 거절하거나 무시해 주세요.";
    let text = format!("{headline}\n\n{body}\n\n확인하기: {url}\n\n--\n{footer}\n");
    let html = format!(
        "<!doctype html><html lang=\"ko\"><body style=\"margin:0;padding:24px;background:#f5f6f8;font-family:-apple-system,'Apple SD Gothic Neo','Malgun Gothic',sans-serif\">\
<div style=\"max-width:520px;margin:0 auto;background:#ffffff;border-radius:16px;padding:28px\">\
<p style=\"margin:0 0 6px;font-size:12px;font-weight:700;letter-spacing:.08em;color:#3b63f6\">AUDENIQ</p>\
<h1 style=\"margin:0 0 14px;font-size:20px;line-height:1.4;color:#111827\">{headline}</h1>\
<p style=\"margin:0 0 20px;line-height:1.6;color:#374151\">{body}</p>\
<a href=\"{url}\" style=\"display:inline-block;padding:12px 20px;border-radius:10px;background:#3b63f6;color:#ffffff;text-decoration:none;font-weight:700\">확인하고 수락하기</a>\
<p style=\"margin:24px 0 0;font-size:12px;line-height:1.6;color:#6b7280\">{footer}</p>\
</div></body></html>",
        headline = escape_html(&headline),
        body = escape_html(&body),
        url = escape_html(&url),
    );
    Rendered {
        subject: format!("[AUDENIQ] {headline}"),
        text,
        html,
    }
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

/// Send waiting invites. Each send makes a new link (only its hash is kept).
pub async fn dispatch_once(pool: &PgPool, keys: &KeyRing, mailer: &Mailer) -> Result<usize> {
    let rows = sqlx::query(
        "UPDATE portal.split_shares sh
            SET invite_attempts=sh.invite_attempts+1, invite_next_at=now() + make_interval(mins => $2)
          WHERE sh.id IN (SELECT id FROM portal.split_shares
                           WHERE invite_status='PENDING' AND invite_next_at <= now()
                           ORDER BY invite_next_at LIMIT $1 FOR UPDATE SKIP LOCKED)
          RETURNING sh.id, sh.invite_attempts, sh.email_cipher, sh.status, sh.share_bps,
            (SELECT o.name FROM identity.orgs o WHERE o.id=sh.org_id) AS org_name,
            (SELECT r.title FROM catalog.releases r WHERE r.id=sh.release_id) AS title,
            (SELECT t.title FROM catalog.tracks t WHERE t.id=sh.track_id) AS track_title",
    )
    .bind(BATCH)
    .bind(LEASE_MINUTES)
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("invite_attempts");
        // Every write below is conditional on the row still being the one we
        // leased: a re-invite in between (new share, attempts reset) wins.
        let done = |status: &'static str| async move {
            sqlx::query("UPDATE portal.split_shares SET invite_status=$2 WHERE id=$1 AND invite_attempts=$3 AND invite_status='PENDING'")
                .bind(id)
                .bind(status)
                .bind(attempts)
                .execute(pool)
                .await
                .map(|_| ())
        };
        let sealed: Option<Vec<u8>> = row.get("email_cipher");
        if row.get::<String, _>("status") != "INVITED" {
            done("SKIPPED").await?;
            continue;
        }
        let Some(Ok(to)) = sealed.map(|c| keys.open_field(Field::SplitEmail, id, &c)) else {
            done("FAILED").await?;
            continue;
        };
        let token = auth::random_token();
        // The link in this mail is the only valid one from now on.
        let current = sqlx::query(
            "UPDATE portal.split_shares SET invite_token_hash=$2
              WHERE id=$1 AND status='INVITED' AND invite_attempts=$3 AND invite_status='PENDING' AND share_bps=$4",
        )
        .bind(id)
        .bind(crate::domain::sha256_hex(&token))
        .bind(attempts)
        .bind(row.get::<i32, _>("share_bps"))
        .execute(pool)
        .await?
        .rows_affected();
        if current != 1 {
            // Changed since we leased it; the new version is sent on its own.
            continue;
        }
        let mail = render(
            &mailer.origin,
            &token,
            &row.get::<String, _>("org_name"),
            &row.get::<String, _>("title"),
            &row.get::<String, _>("track_title"),
            row.get("share_bps"),
        );
        match mailer.send(Uuid::new_v4(), &to, &mail).await {
            Ok(_) => done("SENT").await?,
            Err(Error::PolicyGate("EMAIL_ADDRESS_INVALID")) => done("FAILED").await?,
            Err(_) if attempts >= MAX_ATTEMPTS => done("FAILED").await?,
            Err(_) => {
                sqlx::query("UPDATE portal.split_shares SET invite_next_at=now() + make_interval(secs => $2) WHERE id=$1 AND invite_attempts=$3")
                    .bind(id)
                    .bind(backoff_seconds(attempts))
                    .bind(attempts)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(rows.len())
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_sheet(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(sheet(&s, &a, org, id).await?))
}
async fn h_save(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<SheetInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(save(&s, &a, org, id, i).await?))
}
async fn h_resend(
    State(s): State<AppState>,
    Path((org, id, share)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(resend(&s, &a, org, id, share).await?))
}
async fn h_invite(State(s): State<AppState>, Path(token): Path<String>) -> Result<Json<Value>> {
    Ok(Json(invite(&s, &token).await?))
}
async fn h_accept(
    State(s): State<AppState>,
    Path((org, token)): Path<(Uuid, String)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(accept(&s, &a, org, &token).await?))
}
async fn h_decline(State(s): State<AppState>, Path(token): Path<String>) -> Result<Json<Value>> {
    Ok(Json(decline(&s, &token).await?))
}
async fn h_received(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(received(&s, &a, org).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/releases/{id}/splits",
            get(h_sheet).put(h_save),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/splits/{share}/resend",
            post(h_resend),
        )
        .route("/api/orgs/{org}/splits/received", get(h_received))
        .route("/api/split-invites/{token}", get(h_invite))
        .route("/api/split-invites/{token}/decline", post(h_decline))
        .route(
            "/api/orgs/{org}/split-invites/{token}/accept",
            post(h_accept),
        )
}
