//! Publishing stage 1 (migration 0167, docs/PUBLISHING.md): musical works
//! (작사·작곡 저작물), their contributors and shares, links to recordings,
//! attachments, and the publishing administration application (저작권 관리
//! 신청) with staff review.
//!
//! The Studio screens (web/studio/app/src/api/publishing.ts) call these
//! routes and read the same camelCase shapes as the demo mode, so a screen
//! works the same against either. Contracts, society registration and
//! royalties are stage 2: their list routes answer empty.
//!
//! Access: any ACTIVE member of the org reads; VIEWER cannot write. Staff
//! decide applications through `/api/staff/publishing/*` (review duty). Every
//! mutation is audited as `publishing.*`.
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
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

/// Contributors per work.
const MAX_CONTRIBUTORS: usize = 20;
/// Works per application.
const MAX_APPLICATION_WORKS: usize = 50;
const ROLES: [&str; 4] = ["lyricist", "composer", "arranger", "other"];
const CREATOR_ROLES: [&str; 6] = [
    "artist", "composer", "lyricist", "arranger", "producer", "label",
];
const WORK_TYPES: [&str; 5] = [
    "original",
    "arrangement",
    "translation",
    "instrumental",
    "medley",
];
const LANGUAGES: [&str; 7] = ["ko", "en", "ja", "zh", "es", "zxx", "other"];
const KINDS: [&str; 4] = ["demo", "lyrics", "score", "proof"];
/// Every box of the application's agreement step.
const AGREEMENTS: [&str; 4] = ["terms", "rights", "exclusive", "share"];
/// An application in one of these keeps its works from another application.
const ACTIVE: [&str; 4] = ["submitted", "in_review", "needs_fix", "approved"];
/// Shares can be edited directly only before review or when asked to fix.
const EDITABLE: [&str; 4] = ["none", "preparing", "needs_fix", "rejected"];

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

pub const POLICY_VERSION: &str = "2026-10";
const TERRITORIES: [(&str, &str); 4] = [
    ("KR", "대한민국"),
    ("JP", "일본"),
    ("US", "미국"),
    ("WORLD", "전 세계(그 밖의 국가)"),
];
const RIGHTS: [(&str, &str, &str); 4] = [
    (
        "performance",
        "공연권",
        "방송·공연·매장 음악에서 생기는 사용료",
    ),
    (
        "mechanical",
        "복제권",
        "음반·다운로드 등 복제에서 생기는 사용료",
    ),
    ("digital", "전송권", "스트리밍 서비스에서 생기는 사용료"),
    (
        "sync",
        "싱크 라이선스",
        "영상·광고·게임에 쓰일 때의 이용 허락",
    ),
];

pub fn policy() -> Value {
    json!({
        "commissionRate": 10, "initialTermMonths": 12, "renewalOptions": [6, 12, 36],
        "territories": TERRITORIES.iter().map(|(c, l)| json!({"code": c, "label": l})).collect::<Vec<_>>(),
        "rights": RIGHTS.iter().map(|(k, l, d)| json!({"key": k, "label": l, "desc": d})).collect::<Vec<_>>(),
        "version": POLICY_VERSION,
    })
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// A contributor as the screen sends it. Ids made in the browser are not
/// UUIDs; an id that names a stored contributor of the work keeps its row
/// (and its confirmation while the share is unchanged), anything else is new.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContributorIn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub society: Option<String>,
    #[serde(default)]
    pub ipi: Option<String>,
    #[serde(default)]
    pub share: f64,
    #[serde(default)]
    pub publisher_share: f64,
    #[serde(default)]
    pub is_me: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingIn {
    pub release_id: Uuid,
    pub track_id: Uuid,
}

/// Create (every field) or PATCH (any subset). Unknown fields from the
/// screen (attachments, recording on PATCH) are ignored.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkIn {
    pub title: Option<String>,
    pub alt_titles: Option<Vec<String>>,
    pub work_type: Option<String>,
    pub language: Option<String>,
    pub genre: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub iswc: Option<Option<String>>,
    pub has_prior_publisher: Option<bool>,
    #[serde(default, deserialize_with = "nullable")]
    pub prior_publisher: Option<Option<String>>,
    pub contributors: Option<Vec<ContributorIn>>,
    pub recording: Option<RecordingIn>,
    #[serde(alias = "row_version")]
    pub row_version: Option<i64>,
}

/// `Some(None)` for an explicit null, `None` when the field is absent.
fn nullable<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}

#[derive(Deserialize)]
pub struct RightsIn {
    pub contributors: Vec<ContributorIn>,
    pub row_version: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureIn {
    pub name: String,
    pub method: String,
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationIn {
    pub work_ids: Vec<Uuid>,
    pub territories: Vec<String>,
    pub rights: Vec<String>,
    pub policy_version: String,
    pub agreed: Vec<String>,
    pub signature: SignatureIn,
    pub idempotency_key: String,
}

fn clean(v: &str, max: usize, code: &'static str) -> Result<String> {
    let v = v.trim().to_string();
    if v.chars().count() > max {
        return Err(Error::InvalidCode(code));
    }
    if !v.is_empty() {
        crate::text_policy::check(&v)?;
    }
    Ok(v)
}

fn opt_clean(v: Option<&str>, max: usize, code: &'static str) -> Result<Option<String>> {
    Ok(match v.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => Some(clean(v, max, code)?),
        None => None,
    })
}

/// 33.33% → 3333 basis points.
fn bps(pct: f64) -> Result<i32> {
    if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
        return Err(Error::InvalidCode("WORK_SHARE_INVALID"));
    }
    Ok((pct * 100.0).round() as i32)
}

fn pct(bps: i32) -> f64 {
    f64::from(bps) / 100.0
}

/// A contributor ready to store.
struct Planned {
    keep: Option<Uuid>,
    me: bool,
    name: String,
    roles: Vec<String>,
    society: Option<String>,
    ipi: Option<String>,
    share: i32,
    publisher: i32,
}

fn plan_contributors(list: &[ContributorIn], party: Uuid) -> Result<Vec<Planned>> {
    if list.len() > MAX_CONTRIBUTORS {
        return Err(Error::InvalidCode("WORK_CONTRIBUTORS_TOO_MANY"));
    }
    let mut out = Vec::with_capacity(list.len());
    let mut total = 0;
    let mut me_seen = false;
    let mut kept = HashSet::new();
    for c in list {
        let me = c.is_me == Some(true) || c.creator_id.as_deref() == Some(&party.to_string());
        if me && std::mem::replace(&mut me_seen, true) {
            return Err(Error::InvalidCode("WORK_CONTRIBUTOR_DUPLICATE"));
        }
        let mut roles: Vec<String> = Vec::new();
        for r in &c.roles {
            if !ROLES.contains(&r.as_str()) {
                return Err(Error::InvalidCode("WORK_ROLE_INVALID"));
            }
            if !roles.contains(r) {
                roles.push(r.clone());
            }
        }
        let ipi = match c.ipi.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) if v.len() <= 11 && v.bytes().all(|b| b.is_ascii_digit()) => {
                Some(v.to_string())
            }
            Some(_) => return Err(Error::InvalidCode("WORK_IPI_INVALID")),
            None => None,
        };
        let share = bps(c.share)?;
        let publisher = bps(c.publisher_share)?;
        if publisher > share {
            return Err(Error::InvalidCode("WORK_PUBLISHER_SHARE_OVER"));
        }
        total += share;
        let keep =
            c.id.as_deref()
                .and_then(|v| Uuid::parse_str(v).ok())
                .filter(|id| kept.insert(*id));
        out.push(Planned {
            keep,
            me,
            name: clean(&c.name, 100, "WORK_CONTRIBUTOR_NAME_INVALID")?,
            roles,
            society: opt_clean(c.society.as_deref(), 60, "WORK_SOCIETY_INVALID")?,
            ipi,
            share,
            publisher,
        });
    }
    // Drafts may be under 100% while being split; never over.
    if total > 10_000 {
        return Err(Error::InvalidCode("WORK_SHARE_TOTAL_OVER_100"));
    }
    Ok(out)
}

/// A stored share as submission checks it: name, roles, share, managed share, confirmation.
type ShareRow = (String, Vec<String>, i32, i32, String);

/// What submission needs of a work's shares (the screen's `splitIssues`
/// errors): 100% in total, names and roles, a composer, nobody disputing.
fn split_ready(rows: &[ShareRow]) -> bool {
    !rows.is_empty()
        && rows.iter().map(|r| r.2).sum::<i32>() == 10_000
        && rows.iter().all(|(name, roles, share, publisher, confirm)| {
            !name.trim().is_empty()
                && !roles.is_empty()
                && publisher <= share
                && confirm != "disputed"
        })
        && rows.iter().any(|r| r.1.iter().any(|x| x == "composer"))
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

enum Filter {
    All,
    Ids(Vec<Uuid>),
    Track(Uuid),
}

fn ts(v: DateTime<Utc>) -> String {
    v.to_rfc3339()
}

/// The works of `org` as the screens read them (`Work` in api/publishing.ts).
async fn works_json(
    c: &mut PgConnection,
    org: Uuid,
    party: Uuid,
    role: &str,
    filter: Filter,
) -> Result<Vec<Value>> {
    let (ids, track): (Option<Vec<Uuid>>, Option<Uuid>) = match filter {
        Filter::All => (None, None),
        Filter::Ids(v) => (Some(v), None),
        Filter::Track(t) => (None, Some(t)),
    };
    let works = sqlx::query(
        "SELECT w.id, w.internal_code, w.title, w.alt_titles, w.work_type, w.language, w.genre, w.description, w.visibility,
                w.iswc, w.has_prior_publisher, w.prior_publisher, w.registration, w.fix, w.pending_contributors, w.pending_at,
                w.row_version, w.created_at, w.updated_at
           FROM portal.pub_works w
          WHERE w.org_id=$1 AND w.archived_at IS NULL
            AND ($2::uuid[] IS NULL OR w.id = ANY($2))
            AND ($3::uuid IS NULL OR EXISTS(SELECT 1 FROM portal.pub_work_recordings r WHERE r.work_id=w.id AND r.track_id=$3))
          ORDER BY w.updated_at DESC, w.id",
    )
    .bind(org)
    .bind(ids)
    .bind(track)
    .fetch_all(&mut *c)
    .await?;
    if works.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = works.iter().map(|w| w.get("id")).collect();
    let mut people: HashMap<Uuid, Vec<Value>> = HashMap::new();
    for r in sqlx::query(
        "SELECT id, work_id, party_id, name, roles, society, ipi, share_bps, publisher_share_bps, confirm, external
           FROM portal.pub_work_contributors WHERE work_id = ANY($1) ORDER BY work_id, position",
    )
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?
    {
        let party_id: Option<Uuid> = r.get("party_id");
        people.entry(r.get("work_id")).or_default().push(json!({
            "id": r.get::<Uuid, _>("id"), "creatorId": party_id, "name": r.get::<String, _>("name"),
            "roles": r.get::<Vec<String>, _>("roles"), "society": r.get::<Option<String>, _>("society"),
            "ipi": r.get::<Option<String>, _>("ipi"), "share": pct(r.get("share_bps")),
            "publisherShare": pct(r.get("publisher_share_bps")), "confirm": r.get::<String, _>("confirm"),
            "external": r.get::<bool, _>("external"), "isMe": party_id == Some(party),
        }));
    }
    let mut files: HashMap<Uuid, Vec<Value>> = HashMap::new();
    for r in sqlx::query(
        "SELECT id, work_id, kind, name, size_bytes, text_body, created_at FROM portal.pub_work_attachments
          WHERE work_id = ANY($1) ORDER BY work_id, created_at",
    )
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?
    {
        files.entry(r.get("work_id")).or_default().push(json!({
            "id": r.get::<Uuid, _>("id"), "kind": r.get::<String, _>("kind"), "name": r.get::<String, _>("name"),
            "size": r.get::<Option<i64>, _>("size_bytes"), "at": ts(r.get("created_at")),
            // Stage 1 keeps files in assets without a viewer link yet; text shows in place.
            "url": Value::Null, "text": r.get::<Option<String>, _>("text_body"),
        }));
    }
    let mut recordings: HashMap<Uuid, Vec<Value>> = HashMap::new();
    for r in sqlx::query(
        "SELECT x.id, x.work_id, x.release_id, x.track_id, t.title, r.title AS release_title, t.isrc, lower(r.status) AS status
           FROM portal.pub_work_recordings x
           JOIN catalog.tracks t ON t.id=x.track_id
           JOIN catalog.releases r ON r.id=x.release_id
          WHERE x.work_id = ANY($1) ORDER BY x.work_id, x.created_at",
    )
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?
    {
        recordings.entry(r.get("work_id")).or_default().push(json!({
            "id": r.get::<Uuid, _>("id"), "releaseId": r.get::<Uuid, _>("release_id"), "trackId": r.get::<Uuid, _>("track_id"),
            "title": r.get::<String, _>("title"), "releaseTitle": r.get::<String, _>("release_title"), "version": Value::Null,
            "isrc": r.get::<Option<String>, _>("isrc"), "status": r.get::<String, _>("status"),
        }));
    }
    let mut log: HashMap<Uuid, Vec<Value>> = HashMap::new();
    for r in sqlx::query("SELECT work_id, status, note, society, at FROM portal.pub_work_status_log WHERE work_id = ANY($1) ORDER BY work_id, at, id")
        .bind(&ids)
        .fetch_all(&mut *c)
        .await?
    {
        log.entry(r.get("work_id")).or_default().push(json!({
            "status": r.get::<String, _>("status"), "at": ts(r.get("at")),
            "note": r.get::<Option<String>, _>("note"), "society": r.get::<Option<String>, _>("society"),
        }));
    }
    let mut apps: HashMap<Uuid, Value> = HashMap::new();
    for r in sqlx::query(
        "SELECT DISTINCT ON (aw.work_id) aw.work_id, a.id, a.number, a.status
           FROM portal.pub_application_works aw JOIN portal.pub_applications a ON a.id=aw.application_id
          WHERE aw.work_id = ANY($1) ORDER BY aw.work_id, a.submitted_at DESC, a.id",
    )
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?
    {
        apps.insert(r.get("work_id"), json!({"id": r.get::<Uuid, _>("id"), "number": r.get::<String, _>("number"), "status": r.get::<String, _>("status")}));
    }
    let my_role = if role == "VIEWER" { "viewer" } else { "owner" };
    Ok(works
        .into_iter()
        .map(|w| {
            let id: Uuid = w.get("id");
            let pending = w.get::<Option<Value>, _>("pending_contributors").map(|list| {
                json!({"contributors": list, "requestedAt": w.get::<Option<DateTime<Utc>>, _>("pending_at").map(ts)})
            });
            json!({
                "id": id, "title": w.get::<String, _>("title"), "altTitles": w.get::<Vec<String>, _>("alt_titles"),
                "workType": w.get::<String, _>("work_type"), "language": w.get::<String, _>("language"),
                "genre": w.get::<String, _>("genre"), "description": w.get::<String, _>("description"),
                "visibility": w.get::<String, _>("visibility"), "iswc": w.get::<Option<String>, _>("iswc"),
                "internalCode": w.get::<String, _>("internal_code"), "hasPriorPublisher": w.get::<bool, _>("has_prior_publisher"),
                "priorPublisher": w.get::<Option<String>, _>("prior_publisher"),
                "contributors": people.remove(&id).unwrap_or_default(),
                "attachments": files.remove(&id).unwrap_or_default(),
                "recordings": recordings.remove(&id).unwrap_or_default(),
                "registration": w.get::<String, _>("registration"),
                "registrationLog": log.remove(&id).unwrap_or_default(),
                "application": apps.remove(&id),
                "contractId": Value::Null,
                "fix": w.get::<Option<Value>, _>("fix"),
                "pendingChange": pending,
                "myRole": my_role, "activity": [],
                "rowVersion": w.get::<i64, _>("row_version"),
                "createdAt": ts(w.get("created_at")), "updatedAt": ts(w.get("updated_at")),
            })
        })
        .collect())
}

async fn one(c: &mut PgConnection, org: Uuid, a: &Actor, role: &str, id: Uuid) -> Result<Value> {
    works_json(c, org, a.party, role, Filter::Ids(vec![id]))
        .await?
        .pop()
        .ok_or(Error::NotFound)
}

pub async fn list_works(s: &AppState, a: &Actor, org: Uuid, track: Option<Uuid>) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    let role = auth::membership_read(&mut c, a, org).await?;
    let items = works_json(
        &mut c,
        org,
        a.party,
        &role,
        track.map_or(Filter::All, Filter::Track),
    )
    .await?;
    Ok(json!({"items": items}))
}

pub async fn get_work(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    let role = auth::membership_read(&mut c, a, org).await?;
    one(&mut c, org, a, &role, id).await
}

// ---------------------------------------------------------------------------
// Writing works
// ---------------------------------------------------------------------------

async fn lock_work(c: &mut PgConnection, org: Uuid, id: Uuid) -> Result<(String, i64)> {
    let r = sqlx::query("SELECT registration, row_version FROM portal.pub_works WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR UPDATE")
        .bind(org)
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok((r.get("registration"), r.get("row_version")))
}

async fn touch(c: &mut PgConnection, id: Uuid) -> Result<()> {
    sqlx::query(
        "UPDATE portal.pub_works SET row_version=row_version+1, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .execute(&mut *c)
    .await?;
    Ok(())
}

async fn log_status(
    c: &mut PgConnection,
    org: Uuid,
    work: Uuid,
    status: &str,
    note: Option<&str>,
    society: Option<&str>,
) -> Result<()> {
    sqlx::query("INSERT INTO portal.pub_work_status_log(id,org_id,work_id,status,note,society) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(work)
        .bind(status)
        .bind(note)
        .bind(society)
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// Replace the contributors of a work. Kept rows keep their confirmation
/// while their share and managed share are unchanged; the applicant's own row
/// is confirmed; anyone else changed or new waits for confirmation.
async fn write_contributors(
    c: &mut PgConnection,
    org: Uuid,
    work: Uuid,
    party: Uuid,
    plan: Vec<Planned>,
) -> Result<()> {
    let existing: HashMap<Uuid, (i32, i32, String)> = sqlx::query("SELECT id, share_bps, publisher_share_bps, confirm FROM portal.pub_work_contributors WHERE work_id=$1")
        .bind(work)
        .fetch_all(&mut *c)
        .await?
        .into_iter()
        .map(|r| (r.get("id"), (r.get("share_bps"), r.get("publisher_share_bps"), r.get("confirm"))))
        .collect();
    sqlx::query("DELETE FROM portal.pub_work_contributors WHERE work_id=$1")
        .bind(work)
        .execute(&mut *c)
        .await?;
    for (i, p) in plan.into_iter().enumerate() {
        let prev = p.keep.and_then(|id| existing.get(&id).map(|e| (id, e)));
        let confirm = if p.me {
            "confirmed".to_string()
        } else {
            match prev {
                Some((_, (share, publisher, confirm)))
                    if *share == p.share && *publisher == p.publisher =>
                {
                    confirm.clone()
                }
                _ => "pending".to_string(),
            }
        };
        sqlx::query(
            "INSERT INTO portal.pub_work_contributors(id,org_id,work_id,position,party_id,name,roles,society,ipi,share_bps,publisher_share_bps,confirm,external)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
        )
        .bind(prev.map_or_else(Uuid::new_v4, |(id, _)| id))
        .bind(org)
        .bind(work)
        .bind(i as i32)
        .bind(p.me.then_some(party))
        .bind(&p.name)
        .bind(&p.roles)
        .bind(&p.society)
        .bind(&p.ipi)
        .bind(p.share)
        .bind(p.publisher)
        .bind(confirm)
        .bind(!p.me)
        .execute(&mut *c)
        .await?;
    }
    Ok(())
}

/// The pending contributors of a registered work, as the screen reads them.
fn pending_json(plan: &[Planned], party: Uuid) -> Value {
    Value::Array(
        plan.iter()
            .map(|p| {
                json!({"id": p.keep.unwrap_or_else(Uuid::new_v4), "creatorId": p.me.then_some(party), "name": p.name, "roles": p.roles,
                       "society": p.society, "ipi": p.ipi, "share": pct(p.share), "publisherShare": pct(p.publisher),
                       "confirm": if p.me { "confirmed" } else { "pending" }, "external": !p.me, "isMe": p.me})
            })
            .collect(),
    )
}

fn check_meta(i: &WorkIn) -> Result<()> {
    if let Some(v) = &i.work_type
        && !WORK_TYPES.contains(&v.as_str())
    {
        return Err(Error::InvalidCode("WORK_TYPE_INVALID"));
    }
    if let Some(v) = &i.language
        && !LANGUAGES.contains(&v.as_str())
    {
        return Err(Error::InvalidCode("WORK_LANGUAGE_INVALID"));
    }
    if let Some(v) = &i.visibility
        && !["private", "shared"].contains(&v.as_str())
    {
        return Err(Error::Invalid);
    }
    if let Some(Some(v)) = &i.iswc {
        let digits: String = v.chars().filter(char::is_ascii_digit).collect();
        if !v.trim().is_empty() && (!v.trim_start().starts_with('T') || digits.len() != 10) {
            return Err(Error::InvalidCode("WORK_ISWC_INVALID"));
        }
    }
    if let Some(v) = &i.alt_titles
        && (v.len() > 10 || v.iter().any(|t| t.chars().count() > 300))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// "T3452468001" or "T-345.246.800-1" → "T-345.246.800-1".
fn iswc(v: &str) -> Option<String> {
    let d: String = v.chars().filter(char::is_ascii_digit).collect();
    (d.len() == 10).then(|| format!("T-{}.{}.{}-{}", &d[0..3], &d[3..6], &d[6..9], &d[9..]))
}

async fn link_recording(
    c: &mut PgConnection,
    org: Uuid,
    work: Uuid,
    release: Uuid,
    track: Uuid,
) -> Result<()> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.tracks t JOIN catalog.releases r ON r.id=t.release_id
                        WHERE t.org_id=$1 AND t.id=$2 AND t.release_id=$3 AND t.archived_at IS NULL AND r.archived_at IS NULL)",
    )
    .bind(org)
    .bind(track)
    .bind(release)
    .fetch_one(&mut *c)
    .await?;
    if !ok {
        return Err(Error::InvalidCode("WORK_RECORDING_INVALID"));
    }
    sqlx::query("INSERT INTO portal.pub_work_recordings(id,org_id,work_id,release_id,track_id) VALUES($1,$2,$3,$4,$5) ON CONFLICT (work_id,track_id) DO NOTHING")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(work)
        .bind(release)
        .bind(track)
        .execute(&mut *c)
        .await?;
    Ok(())
}

async fn audit(c: &mut PgConnection, a: &Actor, org: Uuid, id: Uuid, action: &str) -> Result<()> {
    operations::audit(
        c,
        Some(a.user),
        Some(org),
        Some(id),
        action,
        "ARTIST",
        a.request,
    )
    .await
}

pub async fn create_work(s: &AppState, a: &Actor, org: Uuid, i: WorkIn) -> Result<Value> {
    check_meta(&i)?;
    let title = clean(
        i.title.as_deref().unwrap_or_default(),
        300,
        "WORK_TITLE_INVALID",
    )?;
    if title.is_empty() {
        return Err(Error::InvalidCode("WORK_TITLE_INVALID"));
    }
    let plan = plan_contributors(i.contributors.as_deref().unwrap_or_default(), a.party)?;
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.pub_works(id,org_id,internal_code,title,alt_titles,work_type,language,genre,description,visibility,iswc,has_prior_publisher,prior_publisher,created_by)
         VALUES($1,$2,'AQW-'||lpad(nextval('portal.pub_work_code_seq')::text,6,'0'),$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(id)
    .bind(org)
    .bind(&title)
    .bind(i.alt_titles.unwrap_or_default().into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect::<Vec<_>>())
    .bind(i.work_type.as_deref().unwrap_or("original"))
    .bind(i.language.as_deref().unwrap_or("ko"))
    .bind(clean(i.genre.as_deref().unwrap_or_default(), 60, "WORK_GENRE_INVALID")?)
    .bind(clean(i.description.as_deref().unwrap_or_default(), 2000, "WORK_DESCRIPTION_INVALID")?)
    .bind(i.visibility.as_deref().unwrap_or("private"))
    .bind(i.iswc.flatten().as_deref().and_then(iswc))
    .bind(i.has_prior_publisher.unwrap_or(false))
    .bind(opt_clean(i.prior_publisher.flatten().as_deref(), 200, "WORK_PUBLISHER_INVALID")?)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    write_contributors(&mut tx, org, id, a.party, plan).await?;
    if let Some(r) = i.recording {
        link_recording(&mut tx, org, id, r.release_id, r.track_id).await?;
    }
    audit(&mut tx, a, org, id, "publishing.work_created").await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn update_work(s: &AppState, a: &Actor, org: Uuid, id: Uuid, i: WorkIn) -> Result<Value> {
    check_meta(&i)?;
    let title = match i.title.as_deref() {
        Some(t) => {
            let t = clean(t, 300, "WORK_TITLE_INVALID")?;
            if t.is_empty() {
                return Err(Error::InvalidCode("WORK_TITLE_INVALID"));
            }
            Some(t)
        }
        None => None,
    };
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    let (registration, version) = lock_work(&mut tx, org, id).await?;
    if i.row_version.is_some_and(|v| v != version) {
        return Err(Error::Conflict);
    }
    if !EDITABLE.contains(&registration.as_str()) && registration != "registered" {
        return Err(Error::PolicyGate("WORK_UNDER_REVIEW"));
    }
    sqlx::query(
        "UPDATE portal.pub_works SET
           title=COALESCE($3,title), alt_titles=COALESCE($4,alt_titles), work_type=COALESCE($5,work_type),
           language=COALESCE($6,language), genre=COALESCE($7,genre), description=COALESCE($8,description),
           visibility=COALESCE($9,visibility),
           iswc=CASE WHEN $10 THEN $11 ELSE iswc END,
           has_prior_publisher=COALESCE($12,has_prior_publisher),
           prior_publisher=CASE WHEN $13 THEN $14 ELSE prior_publisher END
         WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(id)
    .bind(title)
    .bind(i.alt_titles.map(|v| v.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect::<Vec<_>>()))
    .bind(i.work_type)
    .bind(i.language)
    .bind(i.genre.as_deref().map(|v| clean(v, 60, "WORK_GENRE_INVALID")).transpose()?)
    .bind(i.description.as_deref().map(|v| clean(v, 2000, "WORK_DESCRIPTION_INVALID")).transpose()?)
    .bind(i.visibility)
    .bind(i.iswc.is_some())
    .bind(i.iswc.clone().flatten().as_deref().and_then(iswc))
    .bind(i.prior_publisher.is_some())
    .bind(opt_clean(i.prior_publisher.clone().flatten().as_deref(), 200, "WORK_PUBLISHER_INVALID")?)
    .execute(&mut *tx)
    .await?;
    if let Some(list) = i.contributors {
        let plan = plan_contributors(&list, a.party)?;
        if registration == "registered" {
            return Err(Error::PolicyGate("WORK_RIGHTS_VIA_REVIEW"));
        }
        write_contributors(&mut tx, org, id, a.party, plan).await?;
    }
    touch(&mut tx, id).await?;
    audit(&mut tx, a, org, id, "publishing.work_updated").await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Replace the shares. A registered work keeps its values in force and holds
/// the new ones as a pending change for review.
pub async fn put_rights(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: RightsIn,
) -> Result<Value> {
    let plan = plan_contributors(&i.contributors, a.party)?;
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    let (registration, version) = lock_work(&mut tx, org, id).await?;
    if version != i.row_version {
        return Err(Error::Conflict);
    }
    if registration == "registered" {
        sqlx::query(
            "UPDATE portal.pub_works SET pending_contributors=$2, pending_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(pending_json(&plan, a.party))
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, a, org, id, "publishing.rights_change_requested").await?;
    } else if EDITABLE.contains(&registration.as_str()) {
        write_contributors(&mut tx, org, id, a.party, plan).await?;
        audit(&mut tx, a, org, id, "publishing.rights_saved").await?;
    } else {
        return Err(Error::PolicyGate("WORK_UNDER_REVIEW"));
    }
    touch(&mut tx, id).await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn delete_work(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<()> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    lock_work(&mut tx, org, id).await?;
    let in_use: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.pub_application_works aw JOIN portal.pub_applications x ON x.id=aw.application_id
                        WHERE aw.work_id=$1 AND x.status = ANY($2))",
    )
    .bind(id)
    .bind(ACTIVE.map(String::from).to_vec())
    .fetch_one(&mut *tx)
    .await?;
    if in_use {
        return Err(Error::PolicyGate("WORK_IN_USE"));
    }
    sqlx::query("UPDATE portal.pub_works SET archived_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, a, org, id, "publishing.work_archived").await?;
    tx.commit().await?;
    Ok(())
}

pub async fn add_recording(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    r: RecordingIn,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    lock_work(&mut tx, org, id).await?;
    link_recording(&mut tx, org, id, r.release_id, r.track_id).await?;
    touch(&mut tx, id).await?;
    audit(&mut tx, a, org, id, "publishing.recording_linked").await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn remove_recording(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    rec: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    lock_work(&mut tx, org, id).await?;
    let gone = sqlx::query(
        "DELETE FROM portal.pub_work_recordings WHERE org_id=$1 AND work_id=$2 AND id=$3",
    )
    .bind(org)
    .bind(id)
    .bind(rec)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if gone == 0 {
        return Err(Error::NotFound);
    }
    touch(&mut tx, id).await?;
    audit(&mut tx, a, org, id, "publishing.recording_unlinked").await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Ask a contributor again. Stage 1 records when it was asked; the invite
/// mail is stage 2.
pub async fn request_confirm(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    contributor: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    lock_work(&mut tx, org, id).await?;
    let n = sqlx::query("UPDATE portal.pub_work_contributors SET confirm_requested_at=now() WHERE work_id=$1 AND id=$2 AND confirm <> 'confirmed'")
        .bind(id)
        .bind(contributor)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(Error::NotFound);
    }
    audit(&mut tx, a, org, id, "publishing.confirm_requested").await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

#[derive(Deserialize)]
pub struct AttachmentIn {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub asset_id: Option<Uuid>,
    #[serde(default)]
    pub text: Option<String>,
}

pub async fn add_attachment(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: AttachmentIn,
) -> Result<Value> {
    if !KINDS.contains(&i.kind.as_str()) || i.size.is_some_and(|v| v < 0) {
        return Err(Error::Invalid);
    }
    let name = clean(&i.name, 200, "WORK_FILE_NAME_INVALID")?;
    if name.is_empty() {
        return Err(Error::InvalidCode("WORK_FILE_NAME_INVALID"));
    }
    let text = match i.text.filter(|t| !t.trim().is_empty()) {
        Some(t) if t.chars().count() <= 200_000 => {
            crate::text_policy::check_multiline(&t)?;
            Some(t)
        }
        Some(_) => return Err(Error::InvalidCode("WORK_FILE_TOO_LARGE")),
        None => None,
    };
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    lock_work(&mut tx, org, id).await?;
    if let Some(asset) = i.asset_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog.assets WHERE org_id=$1 AND id=$2)",
        )
        .bind(org)
        .bind(asset)
        .fetch_one(&mut *tx)
        .await?;
        if !ok {
            return Err(Error::InvalidCode("WORK_FILE_INVALID"));
        }
    }
    sqlx::query("INSERT INTO portal.pub_work_attachments(id,org_id,work_id,kind,name,size_bytes,asset_id,text_body,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(id)
        .bind(&i.kind)
        .bind(&name)
        .bind(i.size)
        .bind(i.asset_id)
        .bind(text)
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    touch(&mut tx, id).await?;
    audit(&mut tx, a, org, id, "publishing.file_added").await?;
    let out = one(&mut tx, org, a, &role, id).await?;
    tx.commit().await?;
    Ok(out)
}

#[derive(Deserialize)]
pub struct PrepareIn {
    #[serde(default)]
    pub enroll: bool,
    #[serde(default)]
    pub skip_titles: Vec<String>,
}

/// Make a work for every track of a release that has none, filling the
/// contributors from the track credits (shares split evenly, to be checked).
/// A work of the same title without that recording is linked instead.
pub async fn prepare_from_release(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: PrepareIn,
) -> Result<Value> {
    let skip: HashSet<String> = i
        .skip_titles
        .iter()
        .map(|t| t.trim().to_lowercase())
        .collect();
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let tracks = sqlx::query(
        "SELECT t.id, t.title FROM catalog.tracks t
          WHERE t.org_id=$1 AND t.release_id=$2 AND t.archived_at IS NULL
            AND NOT EXISTS(SELECT 1 FROM portal.pub_work_recordings x JOIN portal.pub_works w ON w.id=x.work_id
                            WHERE x.track_id=t.id AND w.archived_at IS NULL)
          ORDER BY t.disc_number, t.track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let (mut created, mut linked, mut skipped) = (0, 0, 0);
    for t in tracks {
        let track: Uuid = t.get("id");
        let title: String = t.get("title");
        if skip.contains(&title.trim().to_lowercase()) {
            skipped += 1;
            continue;
        }
        let same: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM portal.pub_works WHERE org_id=$1 AND archived_at IS NULL AND lower(btrim(title))=lower(btrim($2)) ORDER BY created_at LIMIT 1",
        )
        .bind(org)
        .bind(&title)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(w) = same {
            link_recording(&mut tx, org, w, release, track).await?;
            touch(&mut tx, w).await?;
            linked += 1;
            continue;
        }
        // Credits per person: the composer/lyricist/arranger roles they hold.
        let mut people: BTreeMap<(String, Uuid), Vec<String>> = BTreeMap::new();
        for r in sqlx::query(
            "SELECT c.party_id, p.display_name, lower(c.role) AS role FROM catalog.credits c JOIN identity.parties p ON p.id=c.party_id
              WHERE c.org_id=$1 AND c.track_id=$2 AND lower(c.role) IN ('composer','lyricist','arranger') ORDER BY p.display_name",
        )
        .bind(org)
        .bind(track)
        .fetch_all(&mut *tx)
        .await?
        {
            people.entry((r.get("display_name"), r.get("party_id"))).or_default().push(r.get("role"));
        }
        let n = people.len() as i32;
        let plan: Vec<Planned> = people
            .into_iter()
            .enumerate()
            .map(|(k, ((name, pid), roles))| {
                // Even split; the last person takes the remainder so it adds up to 100%.
                let share = if n == 0 {
                    0
                } else if k as i32 == n - 1 {
                    10_000 - (10_000 / n) * (n - 1)
                } else {
                    10_000 / n
                };
                Planned {
                    keep: None,
                    me: pid == a.party,
                    name: name.chars().take(100).collect(),
                    roles,
                    society: None,
                    ipi: None,
                    share,
                    publisher: 0,
                }
            })
            .collect();
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO portal.pub_works(id,org_id,internal_code,title,registration,created_by)
             VALUES($1,$2,'AQW-'||lpad(nextval('portal.pub_work_code_seq')::text,6,'0'),$3,$4,$5)",
        )
        .bind(id)
        .bind(org)
        .bind(&title)
        .bind(if i.enroll { "preparing" } else { "none" })
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
        write_contributors(&mut tx, org, id, a.party, plan).await?;
        link_recording(&mut tx, org, id, release, track).await?;
        created += 1;
    }
    audit(&mut tx, a, org, release, "publishing.works_prepared").await?;
    tx.commit().await?;
    Ok(json!({"created": created, "linked": linked, "skipped": skipped}))
}

// ---------------------------------------------------------------------------
// Applications
// ---------------------------------------------------------------------------

async fn applications_json(
    c: &mut PgConnection,
    org: Option<Uuid>,
    ids: Option<Vec<Uuid>>,
    status: Option<&str>,
) -> Result<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT a.id, a.org_id, o.name AS org_name, a.number, a.status, a.territories, a.rights, a.submitted_at, a.fix_reason,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('id',w.id,'title',w.title) ORDER BY w.title)
                            FROM portal.pub_application_works aw JOIN portal.pub_works w ON w.id=aw.work_id WHERE aw.application_id=a.id),'[]') AS works,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('at',e.at,'text',e.text) ORDER BY e.at, e.id)
                            FROM portal.pub_application_events e WHERE e.application_id=a.id),'[]') AS history
           FROM portal.pub_applications a JOIN identity.orgs o ON o.id=a.org_id
          WHERE ($1::uuid IS NULL OR a.org_id=$1) AND ($2::uuid[] IS NULL OR a.id = ANY($2)) AND ($3::text IS NULL OR a.status=$3)
          ORDER BY a.submitted_at DESC, a.id
          LIMIT 200",
    )
    .bind(org)
    .bind(ids)
    .bind(status)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let works: Value = r.get("works");
            let list = works.as_array().cloned().unwrap_or_default();
            json!({
                "id": r.get::<Uuid, _>("id"), "orgId": r.get::<Uuid, _>("org_id"), "orgName": r.get::<String, _>("org_name"),
                "number": r.get::<String, _>("number"), "status": r.get::<String, _>("status"),
                "workIds": list.iter().map(|w| w["id"].clone()).collect::<Vec<_>>(),
                "workTitles": list.iter().map(|w| w["title"].clone()).collect::<Vec<_>>(),
                "territories": r.get::<Vec<String>, _>("territories"), "rights": r.get::<Vec<String>, _>("rights"),
                "submittedAt": ts(r.get("submitted_at")), "contractId": Value::Null,
                "fixReason": r.get::<Option<String>, _>("fix_reason"), "history": r.get::<Value, _>("history"),
            })
        })
        .collect())
}

pub async fn list_applications(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, a, org).await?;
    Ok(json!({"items": applications_json(&mut c, Some(org), None, None).await?}))
}

async fn event(
    c: &mut PgConnection,
    org: Uuid,
    app: Uuid,
    text: &str,
    by: Option<Uuid>,
) -> Result<()> {
    sqlx::query("INSERT INTO portal.pub_application_events(id,org_id,application_id,text,actor_user_id) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(app)
        .bind(text.chars().take(500).collect::<String>())
        .bind(by)
        .execute(&mut *c)
        .await?;
    Ok(())
}

async fn set_registration(
    c: &mut PgConnection,
    org: Uuid,
    works: &[Uuid],
    status: &str,
    fix: Option<&Value>,
    note: Option<&str>,
) -> Result<()> {
    for w in works {
        sqlx::query("UPDATE portal.pub_works SET registration=$2, fix=$3, row_version=row_version+1, updated_at=now() WHERE id=$1")
            .bind(w)
            .bind(status)
            .bind(fix)
            .execute(&mut *c)
            .await?;
        log_status(c, org, *w, status, note, None).await?;
    }
    Ok(())
}

/// Submit an application (신청서 접수). The same idempotency key returns the
/// application already made instead of a second one.
pub async fn submit_application(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    i: ApplicationIn,
) -> Result<Value> {
    if i.policy_version != POLICY_VERSION {
        return Err(Error::PolicyGate("POLICY_CHANGED"));
    }
    let works: Vec<Uuid> = {
        let mut seen = HashSet::new();
        i.work_ids
            .iter()
            .copied()
            .filter(|w| seen.insert(*w))
            .collect()
    };
    if works.is_empty() || works.len() > MAX_APPLICATION_WORKS {
        return Err(Error::InvalidCode("APPLICATION_WORKS_INVALID"));
    }
    let dedup = |v: &[String]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for x in v {
            if !out.contains(x) {
                out.push(x.clone());
            }
        }
        out
    };
    let territories = dedup(&i.territories);
    let rights = dedup(&i.rights);
    if territories.is_empty()
        || territories
            .iter()
            .any(|t| !TERRITORIES.iter().any(|(c, _)| c == t))
    {
        return Err(Error::InvalidCode("APPLICATION_TERRITORY_INVALID"));
    }
    if rights.is_empty()
        || rights
            .iter()
            .any(|r| !RIGHTS.iter().any(|(k, _, _)| k == r))
    {
        return Err(Error::InvalidCode("APPLICATION_RIGHTS_INVALID"));
    }
    if !AGREEMENTS.iter().all(|k| i.agreed.iter().any(|x| x == k)) {
        return Err(Error::InvalidCode("APPLICATION_AGREEMENT_REQUIRED"));
    }
    let sig_name = clean(&i.signature.name, 200, "APPLICATION_SIGNATURE_INVALID")?;
    if sig_name.is_empty() || !["typed", "drawn"].contains(&i.signature.method.as_str()) {
        return Err(Error::InvalidCode("APPLICATION_SIGNATURE_INVALID"));
    }
    let sig_data = match i.signature.data.filter(|d| !d.is_empty()) {
        Some(d) if d.len() <= 300_000 && d.starts_with("data:image/png;base64,") => Some(d),
        Some(_) => return Err(Error::InvalidCode("APPLICATION_SIGNATURE_INVALID")),
        None if i.signature.method == "drawn" => {
            return Err(Error::InvalidCode("APPLICATION_SIGNATURE_INVALID"));
        }
        None => None,
    };
    let key = i.idempotency_key.trim().to_string();
    if !(8..=120).contains(&key.len()) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    // One submission per org at a time, so a resent request finds the first.
    sqlx::query("SELECT 1 FROM identity.orgs WHERE id=$1 FOR UPDATE")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    if let Some(prev) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM portal.pub_applications WHERE org_id=$1 AND idempotency_key=$2",
    )
    .bind(org)
    .bind(&key)
    .fetch_optional(&mut *tx)
    .await?
    {
        let out = applications_json(&mut tx, Some(org), Some(vec![prev]), None)
            .await?
            .pop()
            .ok_or(Error::NotFound)?;
        tx.rollback().await?;
        return Ok(out);
    }
    let rows = sqlx::query("SELECT id, registration FROM portal.pub_works WHERE org_id=$1 AND id = ANY($2) AND archived_at IS NULL ORDER BY id FOR UPDATE")
        .bind(org)
        .bind(&works)
        .fetch_all(&mut *tx)
        .await?;
    if rows.len() != works.len() {
        return Err(Error::NotFound);
    }
    if rows.iter().any(|r| {
        !["none", "preparing", "rejected"].contains(&r.get::<String, _>("registration").as_str())
    }) {
        return Err(Error::PolicyGate("WORK_ALREADY_APPLIED"));
    }
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.pub_application_works aw JOIN portal.pub_applications x ON x.id=aw.application_id
                        WHERE aw.work_id = ANY($1) AND x.status = ANY($2))",
    )
    .bind(&works)
    .bind(ACTIVE.map(String::from).to_vec())
    .fetch_one(&mut *tx)
    .await?;
    if busy {
        return Err(Error::PolicyGate("WORK_ALREADY_APPLIED"));
    }
    let mut shares: HashMap<Uuid, Vec<ShareRow>> = HashMap::new();
    for r in sqlx::query("SELECT work_id, name, roles, share_bps, publisher_share_bps, confirm FROM portal.pub_work_contributors WHERE work_id = ANY($1)")
        .bind(&works)
        .fetch_all(&mut *tx)
        .await?
    {
        shares.entry(r.get("work_id")).or_default().push((r.get("name"), r.get("roles"), r.get("share_bps"), r.get("publisher_share_bps"), r.get("confirm")));
    }
    if works
        .iter()
        .any(|w| !split_ready(shares.get(w).map_or(&[][..], Vec::as_slice)))
    {
        return Err(Error::PolicyGate("WORK_SPLIT_INCOMPLETE"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.pub_applications(id,org_id,number,territories,rights,policy_version,agreed,signature_name,signature_method,signature_data,idempotency_key,submitted_by)
         VALUES($1,$2,'PUB-'||to_char(now() AT TIME ZONE 'Asia/Seoul','YYYY')||'-'||lpad(nextval('portal.pub_application_seq')::text,4,'0'),$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(org)
    .bind(&territories)
    .bind(&rights)
    .bind(POLICY_VERSION)
    .bind(&i.agreed)
    .bind(&sig_name)
    .bind(&i.signature.method)
    .bind(sig_data)
    .bind(&key)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    for w in &works {
        sqlx::query("INSERT INTO portal.pub_application_works(org_id,application_id,work_id) VALUES($1,$2,$3)")
            .bind(org)
            .bind(id)
            .bind(w)
            .execute(&mut *tx)
            .await?;
    }
    set_registration(&mut tx, org, &works, "internal_review", None, None).await?;
    event(&mut tx, org, id, "제출 완료", Some(a.user)).await?;
    audit(&mut tx, a, org, id, "publishing.application_submitted").await?;
    let out = applications_json(&mut tx, Some(org), Some(vec![id]), None)
        .await?
        .pop()
        .ok_or(Error::Internal)?;
    tx.commit().await?;
    Ok(out)
}

async fn lock_application(
    c: &mut PgConnection,
    org: Option<Uuid>,
    id: Uuid,
) -> Result<(Uuid, String, Vec<Uuid>)> {
    let r = sqlx::query("SELECT org_id, status FROM portal.pub_applications WHERE id=$1 AND ($2::uuid IS NULL OR org_id=$2) FOR UPDATE")
        .bind(id)
        .bind(org)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    let works: Vec<Uuid> = sqlx::query_scalar(
        "SELECT work_id FROM portal.pub_application_works WHERE application_id=$1 ORDER BY work_id",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    sqlx::query("SELECT 1 FROM portal.pub_works WHERE id = ANY($1) ORDER BY id FOR UPDATE")
        .bind(&works)
        .execute(&mut *c)
        .await?;
    Ok((r.get("org_id"), r.get("status"), works))
}

pub async fn withdraw_application(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let (_, status, works) = lock_application(&mut tx, Some(org), id).await?;
    if !["submitted", "in_review", "needs_fix"].contains(&status.as_str()) {
        return Err(Error::PolicyGate("APPLICATION_NOT_WITHDRAWABLE"));
    }
    sqlx::query("UPDATE portal.pub_applications SET status='withdrawn' WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    set_registration(&mut tx, org, &works, "none", None, Some("신청 철회")).await?;
    event(&mut tx, org, id, "신청 철회", Some(a.user)).await?;
    audit(&mut tx, a, org, id, "publishing.application_withdrawn").await?;
    let out = applications_json(&mut tx, Some(org), Some(vec![id]), None)
        .await?
        .pop()
        .ok_or(Error::Internal)?;
    tx.commit().await?;
    Ok(out)
}

#[derive(Deserialize)]
pub struct ResubmitIn {
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Answer a fix request: the work goes back to review with the note.
pub async fn resubmit_fix(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    work: Uuid,
    i: ResubmitIn,
) -> Result<Value> {
    let note = clean(&i.note, 1000, "APPLICATION_NOTE_INVALID")?;
    if i.files.len() > 10 || i.files.iter().any(|f| f.chars().count() > 200) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    let (registration, _) = lock_work(&mut tx, org, work).await?;
    if registration != "needs_fix" {
        return Err(Error::PolicyGate("WORK_NO_FIX_REQUESTED"));
    }
    let app: Option<Uuid> = sqlx::query_scalar(
        "SELECT x.id FROM portal.pub_application_works aw JOIN portal.pub_applications x ON x.id=aw.application_id
          WHERE aw.work_id=$1 AND x.status='needs_fix' ORDER BY x.submitted_at DESC LIMIT 1",
    )
    .bind(work)
    .fetch_optional(&mut *tx)
    .await?;
    let shares: Vec<ShareRow> =
        sqlx::query("SELECT name, roles, share_bps, publisher_share_bps, confirm FROM portal.pub_work_contributors WHERE work_id=$1")
            .bind(work)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|r| (r.get("name"), r.get("roles"), r.get("share_bps"), r.get("publisher_share_bps"), r.get("confirm")))
            .collect();
    if !split_ready(&shares) {
        return Err(Error::PolicyGate("WORK_SPLIT_INCOMPLETE"));
    }
    set_registration(
        &mut tx,
        org,
        &[work],
        "internal_review",
        None,
        Some("보완 제출"),
    )
    .await?;
    if let Some(app) = app {
        // Back to review once none of its works still waits for a fix.
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM portal.pub_application_works aw JOIN portal.pub_works w ON w.id=aw.work_id
                            WHERE aw.application_id=$1 AND w.registration='needs_fix')",
        )
        .bind(app)
        .fetch_one(&mut *tx)
        .await?;
        if !waiting {
            sqlx::query("UPDATE portal.pub_applications SET status='in_review', fix_reason=NULL WHERE id=$1").bind(app).execute(&mut *tx).await?;
        }
        let files = if i.files.is_empty() {
            String::new()
        } else {
            format!(" · 첨부 {}", i.files.join(", "))
        };
        let text = if note.is_empty() {
            format!("보완 제출{files}")
        } else {
            format!("보완 제출 — {note}{files}")
        };
        event(&mut tx, org, app, &text, Some(a.user)).await?;
    }
    audit(&mut tx, a, org, work, "publishing.fix_resubmitted").await?;
    let out = one(&mut tx, org, a, &role, work).await?;
    tx.commit().await?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Summary, creators
// ---------------------------------------------------------------------------

pub async fn summary(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    let role = auth::membership_read(&mut c, a, org).await?;
    let works = works_json(&mut c, org, a.party, &role, Filter::All).await?;
    let reg = |w: &Value| w["registration"].as_str().unwrap_or_default().to_string();
    let managed = works
        .iter()
        .filter(|w| {
            ["ready", "submitted", "external_review", "registered"].contains(&reg(w).as_str())
        })
        .count();
    let in_progress = works
        .iter()
        .filter(|w| ["preparing", "internal_review", "needs_fix"].contains(&reg(w).as_str()))
        .count();
    let unregistered = works.iter().filter(|w| reg(w) == "none").count();
    let mut tasks = Vec::new();
    let preparing: Vec<&Value> = works
        .iter()
        .filter(|w| reg(w) == "preparing" && w["application"].is_null())
        .collect();
    if let Some(first) = preparing.first() {
        let release = first["recordings"][0]["releaseId"]
            .as_str()
            .map(|r| format!("?release={r}"))
            .unwrap_or_default();
        tasks.push(json!({"id": "prepare", "kind": "split", "title": format!("발매 곡 {}곡 지분 확인하고 저작권 관리 신청", preparing.len()),
            "sub": "크레딧으로 나눈 지분이 맞는지 확인하면 바로 신청할 수 있어요.", "to": format!("/publishing/apply{release}")}));
    }
    for w in &works {
        let (id, title) = (
            w["id"].as_str().unwrap_or_default(),
            w["title"].as_str().unwrap_or_default(),
        );
        if let Some(reason) = w["fix"]["reason"].as_str() {
            tasks.push(json!({"id": format!("fix-{id}"), "kind": "fix", "title": format!("‘{title}’ 권리 정보 보완 요청"), "sub": reason, "to": format!("/works/{id}?tab=register")}));
        }
        let pending: Vec<&str> = w["contributors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["confirm"] == "pending" && c["isMe"] != true)
            .filter_map(|c| c["name"].as_str())
            .collect();
        if !pending.is_empty() && w["fix"].is_null() {
            tasks.push(json!({"id": format!("split-{id}"), "kind": "split", "title": format!("‘{title}’ 지분 확인을 기다리는 중"),
                "sub": format!("{} 님의 확인이 필요해요.", pending.join(", ")), "to": format!("/works/{id}?tab=rights")}));
        }
    }
    // A released track of this org with no work yet: "이 곡의 저작권도 관리할까요?"
    let suggestion = sqlx::query(
        "SELECT t.id, t.title, r.id AS release_id, r.title AS release_title FROM catalog.tracks t JOIN catalog.releases r ON r.id=t.release_id
          WHERE t.org_id=$1 AND t.archived_at IS NULL AND r.archived_at IS NULL AND r.status NOT IN ('DRAFT','CANCELLED','REJECTED')
            AND NOT EXISTS(SELECT 1 FROM portal.pub_work_recordings x JOIN portal.pub_works w ON w.id=x.work_id WHERE x.track_id=t.id AND w.archived_at IS NULL)
          ORDER BY r.created_at DESC, t.disc_number, t.track_number LIMIT 1",
    )
    .bind(org)
    .fetch_optional(&mut *c)
    .await?
    .map(|r| json!({"releaseId": r.get::<Uuid, _>("release_id"), "trackId": r.get::<Uuid, _>("id"), "title": r.get::<String, _>("title"), "releaseTitle": r.get::<String, _>("release_title")}));
    let enrolled: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM portal.pub_applications WHERE org_id=$1)")
            .bind(org)
            .fetch_one(&mut *c)
            .await?;
    Ok(json!({
        "enrolled": enrolled,
        "works": {"total": works.len(), "managed": managed, "inProgress": in_progress, "unregistered": unregistered},
        "tasks": tasks,
        // Royalties are stage 2.
        "revenue": {"distribution": {"confirmed": 0, "estimated": 0}, "publishing": {"confirmed": 0, "estimated": 0}, "payable": 0},
        "suggestion": suggestion,
    }))
}

/// Writers to add as contributors: the signed-in writer and names already
/// used on this org's works.
pub async fn search_creators(s: &AppState, a: &Actor, org: Uuid, q: &str) -> Result<Value> {
    let q = q.trim();
    if q.chars().count() > 100 {
        return Err(Error::Invalid);
    }
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, a, org).await?;
    let me = my_creator_row(&mut c, a).await?;
    let like = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut items = Vec::new();
    if q.is_empty()
        || me["name"]
            .as_str()
            .is_some_and(|n| n.to_lowercase().contains(&q.to_lowercase()))
        || me["ipi"].as_str() == Some(q)
    {
        items.push(me);
    }
    for r in sqlx::query(
        "SELECT c.name, (array_agg(c.society ORDER BY c.society NULLS LAST))[1] AS society, (array_agg(c.ipi ORDER BY c.ipi NULLS LAST))[1] AS ipi,
                array_agg(DISTINCT r) AS roles, count(DISTINCT c.work_id) AS works
           FROM portal.pub_work_contributors c JOIN portal.pub_works w ON w.id=c.work_id, unnest(c.roles) r
          WHERE c.org_id=$1 AND w.archived_at IS NULL AND c.party_id IS NULL AND c.name <> ''
            AND (c.name ILIKE $2 OR c.ipi = $3)
          GROUP BY c.name ORDER BY count(DISTINCT c.work_id) DESC, c.name LIMIT 20",
    )
    .bind(org)
    .bind(&like)
    .bind(q)
    .fetch_all(&mut *c)
    .await?
    {
        let name: String = r.get("name");
        items.push(json!({"id": format!("name:{name}"), "name": name, "roles": r.get::<Vec<String>, _>("roles"),
            "society": r.get::<Option<String>, _>("society"), "ipi": r.get::<Option<String>, _>("ipi"),
            "linked": false, "workCount": r.get::<i64, _>("works")}));
    }
    Ok(json!({"items": items}))
}

async fn my_creator_row(c: &mut PgConnection, a: &Actor) -> Result<Value> {
    let r = sqlx::query(
        "SELECT p.display_name, cp.roles, cp.society, cp.ipi FROM identity.parties p
           LEFT JOIN portal.pub_creator_profiles cp ON cp.user_id=$2 WHERE p.id=$1",
    )
    .bind(a.party)
    .bind(a.user)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(
        json!({"id": a.party, "name": r.get::<String, _>("display_name"), "legalName": Value::Null,
        "roles": r.get::<Option<Vec<String>>, _>("roles").unwrap_or_default(), "society": r.get::<Option<String>, _>("society"),
        "ipi": r.get::<Option<String>, _>("ipi"), "country": Value::Null, "linked": true, "isMe": true}),
    )
}

pub async fn my_creator(s: &AppState, a: &Actor) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    my_creator_row(&mut c, a).await
}

#[derive(Deserialize)]
pub struct CreatorIn {
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub society: Option<String>,
    #[serde(default)]
    pub ipi: Option<String>,
}

/// Save the writer's IPI, society and roles. The name stays the account's
/// artist name (계정 정보), so it is not taken from here.
pub async fn save_my_creator(s: &AppState, a: &Actor, i: CreatorIn) -> Result<Value> {
    if i.roles.iter().any(|r| !CREATOR_ROLES.contains(&r.as_str())) {
        return Err(Error::InvalidCode("WORK_ROLE_INVALID"));
    }
    let ipi = match i.ipi.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) if v.len() <= 11 && v.bytes().all(|b| b.is_ascii_digit()) => Some(v.to_string()),
        Some(_) => return Err(Error::InvalidCode("WORK_IPI_INVALID")),
        None => None,
    };
    let society = opt_clean(i.society.as_deref(), 60, "WORK_SOCIETY_INVALID")?;
    let mut roles = i.roles;
    roles.dedup();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO portal.pub_creator_profiles(user_id,roles,society,ipi) VALUES($1,$2,$3,$4)
         ON CONFLICT (user_id) DO UPDATE SET roles=EXCLUDED.roles, society=EXCLUDED.society, ipi=EXCLUDED.ipi, updated_at=now()",
    )
    .bind(a.user)
    .bind(&roles)
    .bind(society)
    .bind(ipi)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        None,
        "publishing.creator_saved",
        "ARTIST",
        a.request,
    )
    .await?;
    let out = my_creator_row(&mut tx, a).await?;
    tx.commit().await?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Staff review
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DecisionIn {
    /// start_review · approve · needs_fix · reject · register
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// needs_fix: only these works (default: every work of the application).
    #[serde(default)]
    pub work_ids: Vec<Uuid>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub documents: Vec<String>,
    /// register: the collecting society the works were registered with.
    #[serde(default)]
    pub society: Option<String>,
}

pub async fn staff_applications(
    s: &AppState,
    h: &HeaderMap,
    status: Option<&str>,
) -> Result<Value> {
    staff::staff(s, h, false).await?;
    if status.is_some_and(|v| {
        ![
            "submitted",
            "in_review",
            "needs_fix",
            "approved",
            "rejected",
            "withdrawn",
        ]
        .contains(&v)
    }) {
        return Err(Error::Invalid);
    }
    let mut c = s.pool.acquire().await?;
    Ok(json!({"items": applications_json(&mut c, None, None, status).await?}))
}

/// One application with its works in full, for the reviewer.
pub async fn staff_application(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let mut app = applications_json(&mut c, None, Some(vec![id]), None)
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    let org: Uuid = serde_json::from_value(app["orgId"].clone()).map_err(|_| Error::Internal)?;
    let ids: Vec<Uuid> =
        serde_json::from_value(app["workIds"].clone()).map_err(|_| Error::Internal)?;
    app["works"] =
        Value::Array(works_json(&mut c, org, st.actor.party, "VIEWER", Filter::Ids(ids)).await?);
    Ok(app)
}

pub async fn staff_decide(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionIn) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Review)?;
    let reason = opt_clean(i.reason.as_deref(), 2000, "APPLICATION_REASON_INVALID")?;
    let mut tx = s.pool.begin().await?;
    let (org, status, works) = lock_application(&mut tx, None, id).await?;
    let open = ["submitted", "in_review"].contains(&status.as_str());
    let by = Some(st.actor.user);
    match i.decision.as_str() {
        "start_review" if status == "submitted" => {
            sqlx::query("UPDATE portal.pub_applications SET status='in_review' WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            event(&mut tx, org, id, "심사 시작", by).await?;
        }
        "approve" if open => {
            sqlx::query("UPDATE portal.pub_applications SET status='approved', decided_by=$2, decided_at=now() WHERE id=$1")
                .bind(id)
                .bind(st.actor.user)
                .execute(&mut *tx)
                .await?;
            set_registration(
                &mut tx,
                org,
                &works,
                "ready",
                None,
                Some("권리 정보 확인 완료"),
            )
            .await?;
            event(&mut tx, org, id, "승인", by).await?;
        }
        "needs_fix" if open => {
            let reason = reason
                .clone()
                .ok_or(Error::InvalidCode("APPLICATION_REASON_REQUIRED"))?;
            let target: Vec<Uuid> = if i.work_ids.is_empty() {
                works.clone()
            } else {
                i.work_ids.clone()
            };
            if target.iter().any(|w| !works.contains(w))
                || i.fields.len() > 20
                || i.documents.len() > 10
            {
                return Err(Error::Invalid);
            }
            let documents = i
                .documents
                .iter()
                .map(|d| clean(d, 100, "APPLICATION_REASON_INVALID"))
                .collect::<Result<Vec<_>>>()?;
            let fix = json!({"reason": reason, "fields": i.fields, "documents": documents, "requestedAt": Utc::now().to_rfc3339()});
            sqlx::query(
                "UPDATE portal.pub_applications SET status='needs_fix', fix_reason=$2 WHERE id=$1",
            )
            .bind(id)
            .bind(&reason)
            .execute(&mut *tx)
            .await?;
            set_registration(
                &mut tx,
                org,
                &target,
                "needs_fix",
                Some(&fix),
                Some(&reason),
            )
            .await?;
            event(&mut tx, org, id, "보완 요청", by).await?;
        }
        "reject" if open || status == "needs_fix" => {
            let reason = reason
                .clone()
                .ok_or(Error::InvalidCode("APPLICATION_REASON_REQUIRED"))?;
            sqlx::query("UPDATE portal.pub_applications SET status='rejected', fix_reason=$2, decided_by=$3, decided_at=now() WHERE id=$1")
                .bind(id)
                .bind(&reason)
                .bind(st.actor.user)
                .execute(&mut *tx)
                .await?;
            set_registration(&mut tx, org, &works, "rejected", None, Some(&reason)).await?;
            event(&mut tx, org, id, "반려", by).await?;
        }
        "register" if status == "approved" => {
            let society = opt_clean(i.society.as_deref(), 60, "WORK_SOCIETY_INVALID")?
                .ok_or(Error::Invalid)?;
            for w in &works {
                sqlx::query("UPDATE portal.pub_works SET registration='registered', row_version=row_version+1, updated_at=now() WHERE id=$1 AND registration IN ('ready','submitted','external_review')")
                    .bind(w)
                    .execute(&mut *tx)
                    .await?;
                log_status(
                    &mut tx,
                    org,
                    *w,
                    "registered",
                    Some("곡 코드 발급"),
                    Some(&society),
                )
                .await?;
            }
            event(&mut tx, org, id, &format!("{society} 등록 완료"), by).await?;
        }
        "start_review" | "approve" | "needs_fix" | "reject" | "register" => {
            return Err(Error::PolicyGate("APPLICATION_STATE"));
        }
        _ => return Err(Error::Invalid),
    }
    operations::audit(
        &mut tx,
        by,
        Some(org),
        Some(id),
        &format!("publishing.application_{}", i.decision),
        "STAFF",
        st.actor.request,
    )
    .await?;
    let out = applications_json(&mut tx, None, Some(vec![id]), None)
        .await?
        .pop()
        .ok_or(Error::Internal)?;
    tx.commit().await?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TrackQuery {
    track_id: Option<Uuid>,
}
#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}
#[derive(Deserialize)]
struct StatusQuery {
    status: Option<String>,
}
#[derive(Deserialize)]
struct AgreeIn {
    #[allow(dead_code)]
    agree: bool,
}

async fn reader(s: &AppState, h: &HeaderMap) -> Result<Actor> {
    auth::actor(&s.pool, h, &s.config, false).await
}
async fn writer(s: &AppState, h: &HeaderMap) -> Result<Actor> {
    auth::actor(&s.pool, h, &s.config, true).await
}

async fn h_policy(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = reader(&s, &h).await?;
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, &a, org).await?;
    Ok(Json(policy()))
}
async fn h_summary(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(summary(&s, &reader(&s, &h).await?, org).await?))
}
async fn h_works(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    Query(q): Query<TrackQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        list_works(&s, &reader(&s, &h).await?, org, q.track_id).await?,
    ))
}
async fn h_create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<WorkIn>,
) -> Result<Json<Value>> {
    Ok(Json(create_work(&s, &writer(&s, &h).await?, org, i).await?))
}
async fn h_work(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(get_work(&s, &reader(&s, &h).await?, org, id).await?))
}
async fn h_patch(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<WorkIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        update_work(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_delete(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    delete_work(&s, &writer(&s, &h).await?, org, id).await?;
    Ok(Json(json!({"deleted": true})))
}
async fn h_rights(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<RightsIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        put_rights(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_confirm(
    State(s): State<AppState>,
    Path((org, id, cid)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        request_confirm(&s, &writer(&s, &h).await?, org, id, cid).await?,
    ))
}
/// A contributor answering for themselves needs their own account linked to
/// the work — stage 2 (contributor invites).
async fn h_respond(
    State(s): State<AppState>,
    Path((_org, _id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(_i): Json<AgreeIn>,
) -> Result<Json<Value>> {
    writer(&s, &h).await?;
    Err(Error::Gated)
}
async fn h_link(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<RecordingIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        add_recording(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_unlink(
    State(s): State<AppState>,
    Path((org, id, rec)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        remove_recording(&s, &writer(&s, &h).await?, org, id, rec).await?,
    ))
}
async fn h_attach(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<AttachmentIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        add_attachment(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_resubmit(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<ResubmitIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        resubmit_fix(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_prepare(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PrepareIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        prepare_from_release(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_creators(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    Query(q): Query<SearchQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        search_creators(&s, &reader(&s, &h).await?, org, &q.q).await?,
    ))
}
async fn h_applications(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        list_applications(&s, &reader(&s, &h).await?, org).await?,
    ))
}
async fn h_submit(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ApplicationIn>,
) -> Result<Json<Value>> {
    let a = writer(&s, &h).await?;
    auth::rate(&s.pool, &format!("publishing-apply:{}", a.user), 20).await?;
    Ok(Json(submit_application(&s, &a, org, i).await?))
}
async fn h_withdraw(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        withdraw_application(&s, &writer(&s, &h).await?, org, id).await?,
    ))
}
/// Stage 2 lists (contracts, royalties) answer empty so the screens show
/// their empty state instead of an error.
async fn h_empty(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = reader(&s, &h).await?;
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, &a, org).await?;
    Ok(Json(json!({"items": []})))
}
async fn h_work_royalties(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = reader(&s, &h).await?;
    get_work(&s, &a, org, id).await?;
    Ok(Json(json!({"items": []})))
}
async fn h_me(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(my_creator(&s, &reader(&s, &h).await?).await?))
}
async fn h_me_save(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreatorIn>,
) -> Result<Json<Value>> {
    Ok(Json(save_my_creator(&s, &writer(&s, &h).await?, i).await?))
}
async fn h_staff_list(
    State(s): State<AppState>,
    Query(q): Query<StatusQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_applications(&s, &h, q.status.as_deref()).await?))
}
async fn h_staff_one(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_application(&s, &h, id).await?))
}
async fn h_staff_decide(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionIn>,
) -> Result<Json<Value>> {
    Ok(Json(staff_decide(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/publishing/policy", get(h_policy))
        .route("/api/orgs/{org}/publishing/summary", get(h_summary))
        .route(
            "/api/orgs/{org}/publishing/applications",
            get(h_applications).post(h_submit),
        )
        .route(
            "/api/orgs/{org}/publishing/applications/{id}/withdraw",
            post(h_withdraw),
        )
        .route("/api/orgs/{org}/publishing/contracts", get(h_empty))
        .route("/api/orgs/{org}/publishing/royalties", get(h_empty))
        .route("/api/orgs/{org}/works", get(h_works).post(h_create))
        .route(
            "/api/orgs/{org}/works/{id}",
            get(h_work).patch(h_patch).delete(h_delete),
        )
        .route("/api/orgs/{org}/works/{id}/rights", put(h_rights))
        .route(
            "/api/orgs/{org}/works/{id}/rights/confirmation",
            post(h_respond),
        )
        .route(
            "/api/orgs/{org}/works/{id}/rights/{contributor}/confirmation",
            post(h_confirm),
        )
        .route("/api/orgs/{org}/works/{id}/recordings", post(h_link))
        .route(
            "/api/orgs/{org}/works/{id}/recordings/{rec}",
            axum::routing::delete(h_unlink),
        )
        .route("/api/orgs/{org}/works/{id}/attachments", post(h_attach))
        .route(
            "/api/orgs/{org}/works/{id}/registration/resubmit",
            post(h_resubmit),
        )
        .route(
            "/api/orgs/{org}/works/{id}/royalties",
            get(h_work_royalties),
        )
        .route("/api/orgs/{org}/releases/{id}/works", post(h_prepare))
        .route("/api/orgs/{org}/creators", get(h_creators))
        .route("/api/me/creator", get(h_me).put(h_me_save))
        .route("/api/staff/publishing/applications", get(h_staff_list))
        .route("/api/staff/publishing/applications/{id}", get(h_staff_one))
        .route(
            "/api/staff/publishing/applications/{id}/decision",
            post(h_staff_decide),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, roles: &[&str], share: f64) -> ContributorIn {
        ContributorIn {
            id: None,
            creator_id: None,
            name: name.into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            society: None,
            ipi: None,
            share,
            publisher_share: 0.0,
            is_me: None,
        }
    }

    #[test]
    fn shares_are_basis_points_and_never_over_100() {
        let me = Uuid::new_v4();
        let plan = plan_contributors(
            &[
                person("서린", &["composer"], 33.33),
                person("하늘", &["lyricist"], 66.67),
            ],
            me,
        )
        .unwrap();
        assert_eq!(
            plan.iter().map(|p| p.share).collect::<Vec<_>>(),
            vec![3333, 6667]
        );
        assert!(matches!(
            plan_contributors(
                &[person("a", &["composer"], 60.0), person("b", &[], 50.0)],
                me
            ),
            Err(Error::InvalidCode("WORK_SHARE_TOTAL_OVER_100"))
        ));
        assert!(matches!(
            plan_contributors(&[person("a", &["singer"], 10.0)], me),
            Err(Error::InvalidCode("WORK_ROLE_INVALID"))
        ));
        let mut over = person("a", &["composer"], 10.0);
        over.publisher_share = 20.0;
        assert!(plan_contributors(&[over], me).is_err());
    }

    #[test]
    fn submission_needs_100_percent_roles_and_a_composer() {
        let row = |n: &str, r: &[&str], s: i32, c: &str| {
            (
                n.to_string(),
                r.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
                s,
                0,
                c.to_string(),
            )
        };
        assert!(split_ready(&[
            row("a", &["composer"], 6000, "confirmed"),
            row("b", &["lyricist"], 4000, "pending")
        ]));
        assert!(!split_ready(&[row("a", &["composer"], 9000, "confirmed")]));
        assert!(!split_ready(&[row("a", &["lyricist"], 10000, "confirmed")]));
        assert!(!split_ready(&[
            row("a", &["composer"], 5000, "confirmed"),
            row("b", &["lyricist"], 5000, "disputed")
        ]));
        assert!(!split_ready(&[]));
    }

    #[test]
    fn iswc_is_normalized() {
        assert_eq!(iswc("T3452468001").as_deref(), Some("T-345.246.800-1"));
        assert_eq!(iswc("T-345.246.800-1").as_deref(), Some("T-345.246.800-1"));
        assert_eq!(iswc("T-12"), None);
    }
}
