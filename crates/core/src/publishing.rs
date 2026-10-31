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
//! New tables hold only what distribution has nothing for (works, shares,
//! recordings, attachments, an application's review state). Everything else
//! is the distribution machinery: the signed application is a
//! `portal.release_applications` record (form `PUBLISHING`, sealed), the
//! documents staff ask for are `portal.documents` rights proofs, notices come
//! from `portal.notify`, and an application's history is its audit trail.
//!
//! Access: any ACTIVE member of the org reads; VIEWER cannot write. Staff
//! claim and decide applications through `/api/staff/publishing/*` with the
//! release review's shapes (review duty). Every mutation is audited as
//! `publishing.*`.
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
use sqlx::{Connection, PgConnection, Row};
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
/// Where a work comes from (migration 0169): not released yet (pitched through Open A&R), released by the
/// artist through AUDENIQ (read from the catalog), or released by someone else (given by the artist).
const ORIGINS: [&str; 3] = ["unreleased", "own_release", "other_release"];
/// How AI was used in writing the work (migration 0170); a work made entirely by AI cannot be applied.
const AI_USES: [&str; 3] = ["none", "assisted", "generated"];
/// Every box of the application's agreement step.
/// What the applicant confirms when signing the application (신청서). The contract has its own
/// confirmations (CONTRACT_CONFIRMATIONS) — the application is not the contract, as in distribution.
const AGREEMENTS: [&str; 4] = ["accurate", "rights", "prior", "share"];
/// An application in one of these keeps its works from another application.
const ACTIVE: [&str; 4] = ["submitted", "in_review", "needs_fix", "approved"];
/// Shares can be edited directly only before review or when asked to fix.
const EDITABLE: [&str; 4] = ["none", "preparing", "needs_fix", "rejected"];

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

pub const POLICY_VERSION: &str = "2026-11";
const COMMISSION_BPS: i32 = 1000;
const INITIAL_TERM_MONTHS: i32 = 12;
const RENEWAL_OPTIONS: [i32; 3] = [6, 12, 36];
const TERRITORIES: [(&str, &str); 4] = [
    ("KR", "대한민국"),
    ("JP", "일본"),
    ("US", "미국"),
    ("WORLD", "전 세계(그 밖의 국가)"),
];
const RIGHTS: [(&str, &str, &str); 5] = [
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
    // The composition side of YouTube: Content ID claims on videos that use the song (user uploads, covers, shorts).
    (
        "youtube",
        "유튜브 작곡 권리",
        "유튜브 영상(커버·쇼츠 포함)에 쓰인 곡의 작곡 사용료",
    ),
];

pub fn policy() -> Value {
    json!({
        "commissionRate": COMMISSION_BPS / 100, "initialTermMonths": INITIAL_TERM_MONTHS, "renewalOptions": RENEWAL_OPTIONS,
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
    /// The co-writer's own publisher (0172), if any.
    #[serde(default)]
    pub publisher: Option<String>,
}

/// The original of an arrangement / translation / medley (0172).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OriginalWorkIn {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub writers: String,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub permission: bool,
}

/// A recording sampled or a melody interpolated from another song (0172).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SampleIn {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub owners: Option<String>,
}

fn original_work(v: &OriginalWorkIn) -> Result<Value> {
    let title = clean(&v.title, 300, "WORK_ORIGINAL_INVALID")?;
    if title.is_empty() {
        return Err(Error::InvalidCode("WORK_ORIGINAL_REQUIRED"));
    }
    Ok(
        json!({"title": title, "writers": clean(&v.writers, 300, "WORK_ORIGINAL_INVALID")?,
              "publisher": opt_clean(v.publisher.as_deref(), 200, "WORK_ORIGINAL_INVALID")?.filter(|p| !p.is_empty()),
              "permission": v.permission}),
    )
}

fn samples(list: &[SampleIn]) -> Result<Value> {
    if list.len() > 10 {
        return Err(Error::InvalidCode("WORK_SAMPLES_INVALID"));
    }
    list.iter()
        .map(|x| {
            let title = clean(&x.title, 300, "WORK_SAMPLES_INVALID")?;
            if title.is_empty() || !["sample", "interpolation"].contains(&x.kind.as_str()) {
                return Err(Error::InvalidCode("WORK_SAMPLES_INVALID"));
            }
            Ok(json!({"kind": x.kind, "title": title, "artist": clean(&x.artist, 200, "WORK_SAMPLES_INVALID")?,
                      "owners": opt_clean(x.owners.as_deref(), 300, "WORK_SAMPLES_INVALID")?.filter(|o| !o.is_empty())}))
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn day(v: Option<&str>) -> Result<Option<chrono::NaiveDate>> {
    match v.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| Error::InvalidCode("WORK_DATE_INVALID")),
        None => Ok(None),
    }
}

/// A release of the work that is not in the catalog (someone else released it).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalReleaseIn {
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub isrc: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingIn {
    // The link screen sends snake_case (`release_id`), the work create body camelCase.
    #[serde(alias = "release_id")]
    pub release_id: Uuid,
    #[serde(alias = "track_id")]
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
    pub origin: Option<String>,
    pub ai_use: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub original_work: Option<Option<OriginalWorkIn>>,
    pub samples: Option<Vec<SampleIn>>,
    #[serde(default, deserialize_with = "nullable")]
    pub prior_publisher_until: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    pub external_release: Option<Option<ExternalReleaseIn>>,
    #[serde(alias = "row_version")]
    pub row_version: Option<i64>,
}

/// `Some(None)` for an explicit null, `None` when the field is absent.
fn nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::<T>::deserialize(d)?))
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
    publisher_name: Option<String>,
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
            publisher_name: opt_clean(c.publisher.as_deref(), 200, "WORK_PUBLISHER_INVALID")?
                .filter(|v| !v.is_empty()),
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

/// Fields a reviewer can ask to fix on a song (`fields` of a correction request) and how Studio names them.
/// Same keys as FIX_FIELDS in web/studio/app/src/lib/publishing.ts.
fn fix_field_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "contributors.split" => "참여자·지분",
        "contributors.ipi" => "참여자 IPI 번호",
        "contributors.society" => "저작권 관리 단체",
        "work.info" => "곡 정보",
        "work.prior" => "기존 퍼블리셔",
        _ => return None,
    })
}

/// One line for a correction request: what to fix and which documents, or the reviewer's comment when
/// nothing was picked ('참여자 IPI 번호 · 서류 공동 작곡 합의서').
fn fix_summary(fields: &[String], documents: &[String], comment: &str) -> String {
    let mut parts: Vec<String> = fields
        .iter()
        .filter_map(|f| fix_field_label(f))
        .map(str::to_string)
        .collect();
    if !documents.is_empty() {
        parts.push(format!("서류 {}", documents.join(", ")));
    }
    if parts.is_empty() {
        comment.to_string()
    } else {
        format!("{} 보완", parts.join(" · "))
    }
}

/// Studio's release status chip (draft, review, needs, scheduled, live, ...) for a release pipeline status.
/// Mirrors `uiStatus` in web/studio/app/src/api/remote.ts so a linked recording reads like its release
/// instead of a raw stage name such as `stage1_correction`.
fn release_chip(pipeline: &str, live: bool, agreement: Option<&str>) -> &'static str {
    if agreement == Some("NEEDS") {
        return "needs";
    }
    if live && agreement == Some("SIGNED") {
        return "live";
    }
    match pipeline {
        "DRAFT" => "draft",
        "WITHDRAWN" if agreement == Some("CANCELLED") => "cancelled",
        "WITHDRAWN" => "rejected",
        "SUPERSEDED" => "closed",
        s if s.ends_with("CORRECTION") || s == "ON_HOLD_RIGHTS" => "needs",
        "READY_FOR_DELIVERY" if matches!(agreement, Some("APPROVED" | "SIGNED")) => "scheduled",
        _ => "review",
    }
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
                w.origin, w.external_release, w.ai_use, w.original_work, w.samples, w.prior_publisher_until, w.row_version, w.created_at, w.updated_at,
                -- The signed contract the work is under (a contract waiting for signature does not count yet).
                (SELECT c.id FROM portal.pub_contracts c JOIN portal.pub_application_works aw ON aw.application_id=c.application_id
                  WHERE aw.work_id=w.id AND c.status <> 'pending_sign' ORDER BY c.created_at DESC LIMIT 1) AS contract_id
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
        "SELECT c.id, c.work_id, c.party_id, c.name, c.roles, c.society, c.ipi, c.share_bps, c.publisher_share_bps, c.confirm, c.external, c.publisher_name,
                i.note AS invite_note, (i.id IS NOT NULL AND i.answer IS NULL) AS invited, i.invite_status
           FROM portal.pub_work_contributors c
           LEFT JOIN portal.pub_split_invites i ON i.work_id=c.work_id AND i.contributor_id=c.id
          WHERE c.work_id = ANY($1) ORDER BY c.work_id, c.position",
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
            // Co-writer invite (0171): a link is out, mailed or not, and what they wrote when disputing.
            "publisher": r.get::<Option<String>, _>("publisher_name"),
            "invited": r.get::<bool, _>("invited"), "inviteNote": r.get::<Option<String>, _>("invite_note"),
            "inviteMail": r.get::<Option<String>, _>("invite_status").filter(|v| v != "NONE"),
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
    let rows = sqlx::query(
        "SELECT x.id, x.work_id, x.release_id, x.track_id, t.title, r.title AS release_title, t.isrc, r.status
           FROM portal.pub_work_recordings x
           JOIN catalog.tracks t ON t.id=x.track_id
           JOIN catalog.releases r ON r.id=x.release_id
          WHERE x.work_id = ANY($1) ORDER BY x.work_id, x.created_at",
    )
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?;
    // Same chip as the release list: the agreement and live delivery decide it, not the pipeline stage alone.
    // Live bindings are org-scoped (RLS), so read them in a transaction that sets app.org_id.
    let releases: Vec<Uuid> = rows
        .iter()
        .map(|r| r.get::<Uuid, _>("release_id"))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let (agreements, mut axes) = if releases.is_empty() {
        (HashMap::new(), HashMap::new())
    } else {
        let mut tx = c.begin().await?;
        let agreements = crate::catalog::agreement_statuses(&mut tx, org, &releases).await?;
        let axes = crate::catalog::delivery_axes(&mut tx, org, &releases).await?;
        tx.commit().await?;
        (agreements, axes)
    };
    let live: HashSet<Uuid> = axes
        .drain()
        .filter(|(_, (_, live))| live.iter().any(|l| l["live_status"] == "LIVE"))
        .map(|(id, _)| id)
        .collect();
    for r in rows {
        let release: Uuid = r.get("release_id");
        let status = release_chip(
            &r.get::<String, _>("status"),
            live.contains(&release),
            agreements.get(&release).map(String::as_str),
        );
        recordings.entry(r.get("work_id")).or_default().push(json!({
            "id": r.get::<Uuid, _>("id"), "releaseId": release, "trackId": r.get::<Uuid, _>("track_id"),
            "title": r.get::<String, _>("title"), "releaseTitle": r.get::<String, _>("release_title"), "version": Value::Null,
            "isrc": r.get::<Option<String>, _>("isrc"), "status": status,
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
        "SELECT DISTINCT ON (aw.work_id) aw.work_id, a.id, ra.application_no AS number, a.status
           FROM portal.pub_application_works aw JOIN portal.pub_applications a ON a.id=aw.application_id
           JOIN portal.release_applications ra ON ra.id=a.record_id
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
                "origin": w.get::<String, _>("origin"),
                "externalRelease": w.get::<Option<Value>, _>("external_release"),
                "aiUse": w.get::<String, _>("ai_use"),
                "originalWork": w.get::<Option<Value>, _>("original_work"),
                "samples": w.get::<Value, _>("samples"),
                "priorPublisherUntil": w.get::<Option<chrono::NaiveDate>, _>("prior_publisher_until").map(|d| d.to_string()),
                "contributors": people.remove(&id).unwrap_or_default(),
                "attachments": files.remove(&id).unwrap_or_default(),
                "recordings": recordings.remove(&id).unwrap_or_default(),
                "registration": w.get::<String, _>("registration"),
                "registrationLog": log.remove(&id).unwrap_or_default(),
                "application": apps.remove(&id),
                "contractId": w.get::<Option<Uuid>, _>("contract_id"),
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
    user: Uuid,
    mut plan: Vec<Planned>,
) -> Result<()> {
    // The writer's own society and IPI are typed once: an empty one on the writer's row is filled from the
    // profile (계정 정보), and one typed on a work fills an empty profile, so the next work has it already.
    // A profile value is never overwritten from a work — that stays a choice in 계정 정보.
    if let Some(me) = plan.iter_mut().find(|p| p.me) {
        let (society, ipi): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT society, ipi FROM portal.pub_creator_profiles WHERE user_id=$1")
                .bind(user)
                .fetch_optional(&mut *c)
                .await?
                .unwrap_or((None, None));
        let learn =
            (society.is_none() && me.society.is_some()) || (ipi.is_none() && me.ipi.is_some());
        if learn {
            sqlx::query(
                "INSERT INTO portal.pub_creator_profiles(user_id,roles,society,ipi) VALUES($1,'{}',$2,$3)
                 ON CONFLICT (user_id) DO UPDATE SET society=COALESCE(portal.pub_creator_profiles.society, EXCLUDED.society),
                   ipi=COALESCE(portal.pub_creator_profiles.ipi, EXCLUDED.ipi), updated_at=now()",
            )
            .bind(user)
            .bind(&me.society)
            .bind(&me.ipi)
            .execute(&mut *c)
            .await?;
        }
        if me.society.is_none() {
            me.society = society;
        }
        if me.ipi.is_none() {
            me.ipi = ipi;
        }
    }
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
            "INSERT INTO portal.pub_work_contributors(id,org_id,work_id,position,party_id,name,roles,society,ipi,share_bps,publisher_share_bps,confirm,external,publisher_name)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
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
        .bind(&p.publisher_name)
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
                       "confirm": if p.me { "confirmed" } else { "pending" }, "external": !p.me, "isMe": p.me, "publisher": p.publisher_name})
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
    if let Some(v) = &i.origin
        && !ORIGINS.contains(&v.as_str())
    {
        return Err(Error::InvalidCode("WORK_ORIGIN_INVALID"));
    }
    if let Some(v) = &i.ai_use
        && !AI_USES.contains(&v.as_str())
    {
        return Err(Error::InvalidCode("WORK_AI_USE_INVALID"));
    }
    Ok(())
}

/// The release someone else made, as stored: artist and title are needed, the rest is optional.
fn external_release(v: &ExternalReleaseIn) -> Result<Value> {
    let artist = clean(&v.artist, 200, "WORK_RELEASE_INFO_INVALID")?;
    let title = clean(&v.title, 300, "WORK_RELEASE_INFO_INVALID")?;
    if artist.is_empty() || title.is_empty() {
        return Err(Error::InvalidCode("WORK_RELEASE_INFO_REQUIRED"));
    }
    let date = opt_clean(v.release_date.as_deref(), 10, "WORK_RELEASE_INFO_INVALID")?
        .filter(|d| !d.is_empty());
    if let Some(d) = &date
        && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err()
    {
        return Err(Error::InvalidCode("WORK_RELEASE_INFO_INVALID"));
    }
    let isrc = opt_clean(v.isrc.as_deref(), 20, "WORK_RELEASE_INFO_INVALID")?
        .map(|x| x.replace('-', "").to_uppercase())
        .filter(|x| !x.is_empty());
    if let Some(x) = &isrc
        && (x.len() != 12 || !x.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return Err(Error::InvalidCode("WORK_RELEASE_INFO_INVALID"));
    }
    let label =
        opt_clean(v.label.as_deref(), 200, "WORK_RELEASE_INFO_INVALID")?.filter(|x| !x.is_empty());
    Ok(json!({"artist": artist, "title": title, "releaseDate": date, "isrc": isrc, "label": label}))
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
    // A song not released yet that now has a recording of the org's own release is released by the artist.
    sqlx::query(
        "UPDATE portal.pub_works SET origin='own_release' WHERE id=$1 AND origin='unreleased'",
    )
    .bind(work)
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
    // A recording of the org's own release decides it; otherwise what the artist picked.
    let origin = if i.recording.is_some() {
        "own_release"
    } else {
        i.origin.as_deref().unwrap_or("unreleased")
    };
    let external = match (origin, i.external_release.clone().flatten()) {
        ("other_release", Some(v)) => Some(external_release(&v)?),
        ("other_release", None) => return Err(Error::InvalidCode("WORK_RELEASE_INFO_REQUIRED")),
        _ => None,
    };
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.pub_works(id,org_id,internal_code,title,alt_titles,work_type,language,genre,description,visibility,iswc,has_prior_publisher,prior_publisher,created_by,origin,external_release,ai_use,original_work,samples,prior_publisher_until)
         VALUES($1,$2,'AQW-'||lpad(nextval('portal.pub_work_code_seq')::text,6,'0'),$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
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
    .bind(origin)
    .bind(external)
    .bind(i.ai_use.as_deref().unwrap_or("none"))
    .bind(i.original_work.clone().flatten().map(|o| original_work(&o)).transpose()?)
    .bind(samples(i.samples.as_deref().unwrap_or_default())?)
    .bind(day(i.prior_publisher_until.clone().flatten().as_deref())?)
    .execute(&mut *tx)
    .await?;
    write_contributors(&mut tx, org, id, a.party, a.user, plan).await?;
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
    // A pitched song released by someone else moves to other_release with that release; own_release comes only
    // from a linked recording, so the screen cannot claim it.
    if i.origin.as_deref() == Some("own_release") {
        return Err(Error::InvalidCode("WORK_ORIGIN_INVALID"));
    }
    let external = match i.external_release.clone() {
        Some(Some(v)) => Some(Some(external_release(&v)?)),
        Some(None) => Some(None),
        None => None,
    };
    if i.origin.as_deref() == Some("other_release") {
        let given = match &external {
            Some(v) => v.is_some(),
            None => {
                sqlx::query_scalar(
                    "SELECT external_release IS NOT NULL FROM portal.pub_works WHERE id=$1",
                )
                .bind(id)
                .fetch_one(&mut *tx)
                .await?
            }
        };
        if !given {
            return Err(Error::InvalidCode("WORK_RELEASE_INFO_REQUIRED"));
        }
    }
    sqlx::query(
        "UPDATE portal.pub_works SET
           title=COALESCE($3,title), alt_titles=COALESCE($4,alt_titles), work_type=COALESCE($5,work_type),
           language=COALESCE($6,language), genre=COALESCE($7,genre), description=COALESCE($8,description),
           visibility=COALESCE($9,visibility),
           iswc=CASE WHEN $10 THEN $11 ELSE iswc END,
           has_prior_publisher=COALESCE($12,has_prior_publisher),
           prior_publisher=CASE WHEN $13 THEN $14 ELSE prior_publisher END,
           origin=COALESCE($15,origin),
           external_release=CASE WHEN $16 THEN $17 ELSE external_release END,
           ai_use=COALESCE($18,ai_use),
           original_work=CASE WHEN $19 THEN $20 ELSE original_work END,
           samples=COALESCE($21,samples),
           prior_publisher_until=CASE WHEN $22 THEN $23 ELSE prior_publisher_until END
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
    .bind(i.origin.as_deref())
    .bind(external.is_some())
    .bind(external.flatten())
    .bind(i.ai_use.as_deref())
    .bind(i.original_work.is_some())
    .bind(i.original_work.clone().flatten().map(|o| original_work(&o)).transpose()?)
    .bind(i.samples.as_deref().map(samples).transpose()?)
    .bind(i.prior_publisher_until.is_some())
    .bind(day(i.prior_publisher_until.clone().flatten().as_deref())?)
    .execute(&mut *tx)
    .await?;
    if let Some(list) = i.contributors {
        let plan = plan_contributors(&list, a.party)?;
        if registration == "registered" {
            return Err(Error::PolicyGate("WORK_RIGHTS_VIA_REVIEW"));
        }
        write_contributors(&mut tx, org, id, a.party, a.user, plan).await?;
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
    // A work under a signed contract is in force before the society registers it too: its shares change
    // only through review, as for a registered work (the screen asks for a change in both cases).
    let contracted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.pub_contracts c JOIN portal.pub_application_works aw ON aw.application_id=c.application_id
                        WHERE aw.work_id=$1 AND c.status <> 'pending_sign')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if registration == "registered" || (contracted && !EDITABLE.contains(&registration.as_str())) {
        sqlx::query(
            "UPDATE portal.pub_works SET pending_contributors=$2, pending_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(pending_json(&plan, a.party))
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, a, org, id, "publishing.rights_change_requested").await?;
    } else if EDITABLE.contains(&registration.as_str()) {
        write_contributors(&mut tx, org, id, a.party, a.user, plan).await?;
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

// ---------------------------------------------------------------------------
// Open A&R (migration 0172)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArIn {
    pub work_id: Uuid,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub mood: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub references: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub sync: bool,
    #[serde(default)]
    pub consent: bool,
    #[serde(default)]
    pub idempotency_key: String,
}

/// What happened to a submission, from the audit trail (as applications).
const AR_HISTORY: &str = "COALESCE((SELECT jsonb_agg(jsonb_build_object('at',e.occurred_at,'text',
      CASE e.action WHEN 'publishing.ar_submitted' THEN '제출 완료' WHEN 'publishing.ar_withdrawn' THEN '제출 취소'
        WHEN 'publishing.ar_review' THEN '심사 시작' WHEN 'publishing.ar_fix' THEN '보완 요청' WHEN 'publishing.ar_reviewed' THEN '심사 완료'
        WHEN 'publishing.ar_proposing' THEN '제안 진행' END) ORDER BY e.occurred_at, e.id)
    FROM operations.audit_events e WHERE e.resource_id=s.id AND e.action LIKE 'publishing.ar_%'),'[]')";

async fn ar_json(c: &mut PgConnection, org: Uuid, id: Option<Uuid>) -> Result<Vec<Value>> {
    // Only constant text is formatted in (AR_HISTORY); every value is bound.
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT s.id, s.work_id, w.title, s.genre, s.mood, s.description, s.refs, s.scope, s.sync, s.status, s.fix_reason, s.submitted_at,
                {AR_HISTORY} AS history
           FROM portal.pub_ar_submissions s JOIN portal.pub_works w ON w.id=s.work_id
          WHERE s.org_id=$1 AND ($2::uuid IS NULL OR s.id=$2) ORDER BY s.submitted_at DESC, s.id"
    )))
    .bind(org)
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            json!({"id": r.get::<Uuid, _>("id"), "workId": r.get::<Uuid, _>("work_id"), "workTitle": r.get::<String, _>("title"),
                   "genre": r.get::<String, _>("genre"), "mood": r.get::<Vec<String>, _>("mood"), "description": r.get::<String, _>("description"),
                   "references": r.get::<String, _>("refs"), "scope": r.get::<String, _>("scope"), "sync": r.get::<bool, _>("sync"),
                   "status": r.get::<String, _>("status"), "fixReason": r.get::<Option<String>, _>("fix_reason"),
                   "submittedAt": ts(r.get("submitted_at")), "history": r.get::<Value, _>("history")})
        })
        .collect())
}

pub async fn list_ar(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, a, org).await?;
    Ok(json!({"items": ar_json(&mut c, org, None).await?}))
}

/// Send a demo of a song not released yet to A&R. Released songs are administered, not pitched.
pub async fn submit_ar(s: &AppState, a: &Actor, org: Uuid, i: ArIn) -> Result<Value> {
    if !i.consent {
        return Err(Error::InvalidCode("AR_CONSENT_REQUIRED"));
    }
    let key = i.idempotency_key.trim().to_string();
    if !(8..=120).contains(&key.len()) {
        return Err(Error::Invalid);
    }
    let genre = clean(&i.genre, 60, "AR_GENRE_INVALID")?;
    if genre.is_empty() {
        return Err(Error::InvalidCode("AR_GENRE_INVALID"));
    }
    let mut mood: Vec<String> = Vec::new();
    for m in &i.mood {
        let m = clean(m, 20, "AR_MOOD_INVALID")?;
        if !m.is_empty() && !mood.contains(&m) {
            mood.push(m);
        }
    }
    if mood.is_empty() || mood.len() > 9 {
        return Err(Error::InvalidCode("AR_MOOD_INVALID"));
    }
    if !["internal", "partners"].contains(&i.scope.as_str()) {
        return Err(Error::Invalid);
    }
    let description = clean(&i.description, 2000, "AR_DESCRIPTION_INVALID")?;
    let refs = clean(&i.references, 500, "AR_REFERENCES_INVALID")?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    if let Some(prev) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM portal.pub_ar_submissions WHERE org_id=$1 AND idempotency_key=$2",
    )
    .bind(org)
    .bind(&key)
    .fetch_optional(&mut *tx)
    .await?
    {
        let out = ar_json(&mut tx, org, Some(prev))
            .await?
            .pop()
            .ok_or(Error::NotFound)?;
        tx.rollback().await?;
        return Ok(out);
    }
    lock_work(&mut tx, org, i.work_id).await?;
    let (released, demo, open): (bool, bool, bool) = sqlx::query_as(
        "SELECT w.origin <> 'unreleased' OR EXISTS(SELECT 1 FROM portal.pub_work_recordings r WHERE r.work_id=w.id),
                EXISTS(SELECT 1 FROM portal.pub_work_attachments f WHERE f.work_id=w.id AND f.kind='demo'),
                EXISTS(SELECT 1 FROM portal.pub_ar_submissions x WHERE x.work_id=w.id AND x.status <> 'closed')
           FROM portal.pub_works w WHERE w.org_id=$1 AND w.id=$2",
    )
    .bind(org)
    .bind(i.work_id)
    .fetch_one(&mut *tx)
    .await?;
    if released {
        return Err(Error::PolicyGate("WORK_RELEASED"));
    }
    if !demo {
        return Err(Error::PolicyGate("AR_DEMO_REQUIRED"));
    }
    if open {
        return Err(Error::PolicyGate("DUPLICATE_SUBMISSION"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.pub_ar_submissions(id,org_id,work_id,genre,mood,description,refs,scope,sync,idempotency_key,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(org)
    .bind(i.work_id)
    .bind(&genre)
    .bind(&mood)
    .bind(&description)
    .bind(&refs)
    .bind(&i.scope)
    .bind(i.sync)
    .bind(&key)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, a, org, id, "publishing.ar_submitted").await?;
    touch(&mut tx, i.work_id).await?;
    let out = ar_json(&mut tx, org, Some(id))
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(out)
}

pub async fn withdraw_ar(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let n = sqlx::query("UPDATE portal.pub_ar_submissions SET status='closed' WHERE org_id=$1 AND id=$2 AND status <> 'closed'")
        .bind(org)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    audit(&mut tx, a, org, id, "publishing.ar_withdrawn").await?;
    let out = ar_json(&mut tx, org, Some(id))
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Co-writer invites (migration 0171)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct InviteIn {
    #[serde(default)]
    pub email: Option<String>,
}

/// A link for a co-writer to confirm their share — copied by the applicant and/or mailed. Asking again makes a
/// new link (the old one stops working). Answers set the contributor's confirm.
pub async fn invite_contributor(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    contributor: Uuid,
    i: InviteIn,
) -> Result<Value> {
    let email = match i.email.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => Some(
            crate::email::normalize_address(v)
                .ok_or(Error::InvalidCode("WORK_INVITE_EMAIL_INVALID"))?,
        ),
        None => None,
    };
    auth::rate(&s.pool, &format!("pub-invite:{contributor}"), 5).await?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    lock_work(&mut tx, org, id).await?;
    let row: Option<(Option<Uuid>, String)> = sqlx::query_as(
        "SELECT party_id, confirm FROM portal.pub_work_contributors WHERE work_id=$1 AND id=$2",
    )
    .bind(id)
    .bind(contributor)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((party, confirm)) = row else {
        return Err(Error::NotFound);
    };
    if party == Some(a.party) {
        return Err(Error::PolicyGate("WORK_INVITE_SELF"));
    }
    if confirm == "confirmed" {
        return Err(Error::PolicyGate("WORK_ALREADY_CONFIRMED"));
    }
    let token = auth::random_token();
    let invite: Uuid = sqlx::query_scalar(
        "INSERT INTO portal.pub_split_invites(id,org_id,work_id,contributor_id,token_hash,created_by) VALUES($1,$2,$3,$4,$5,$6)
         ON CONFLICT (work_id, contributor_id) DO UPDATE SET token_hash=EXCLUDED.token_hash, email_cipher=NULL, token_cipher=NULL,
           invite_status='NONE', invite_attempts=0, answer=NULL, note=NULL, answered_at=NULL, created_by=EXCLUDED.created_by, created_at=now()
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(id)
    .bind(contributor)
    .bind(crate::domain::sha256_hex(&token))
    .bind(a.user)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(to) = &email {
        let keys = &s.payout_keys;
        sqlx::query("UPDATE portal.pub_split_invites SET email_cipher=$2, token_cipher=$3, invite_status='PENDING', invite_next_at=now() WHERE id=$1")
            .bind(invite)
            .bind(keys.seal_field(crate::payout_keys::Field::PubInviteEmail, invite, to)?)
            .bind(keys.seal_field(crate::payout_keys::Field::PubInviteToken, invite, &token)?)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE portal.pub_work_contributors SET confirm='pending', confirm_requested_at=now() WHERE work_id=$1 AND id=$2")
        .bind(id)
        .bind(contributor)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, a, org, id, "publishing.confirm_invited").await?;
    tx.commit().await?;
    Ok(json!({"token": token, "emailed": email.is_some()}))
}

async fn invite_by_token(c: &mut PgConnection, token: &str) -> Result<sqlx::postgres::PgRow> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::NotFound);
    }
    sqlx::query(
        "SELECT i.id, i.org_id, i.work_id, i.contributor_id, i.answer, w.title, o.name AS org_name
           FROM portal.pub_split_invites i
           JOIN portal.pub_works w ON w.id=i.work_id AND w.archived_at IS NULL
           JOIN identity.orgs o ON o.id=i.org_id
           JOIN portal.pub_work_contributors c ON c.work_id=i.work_id AND c.id=i.contributor_id
          WHERE i.token_hash=$1 FOR UPDATE OF i",
    )
    .bind(crate::domain::sha256_hex(token.to_ascii_lowercase()))
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

/// What the co-writer is asked to confirm (no account: the link is the secret).
pub async fn split_invite(s: &AppState, token: &str) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let r = invite_by_token(&mut tx, token).await?;
    let me: Uuid = r.get("contributor_id");
    let people: Vec<Value> = sqlx::query(
        "SELECT id, name, roles, share_bps FROM portal.pub_work_contributors WHERE work_id=$1 ORDER BY position",
    )
    .bind(r.get::<Uuid, _>("work_id"))
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|p| {
        json!({"name": p.get::<String, _>("name"), "roles": p.get::<Vec<String>, _>("roles"),
               "share": pct(p.get("share_bps")), "you": p.get::<Uuid, _>("id") == me})
    })
    .collect();
    tx.rollback().await?;
    Ok(json!({
        "title": r.get::<String, _>("title"), "from": r.get::<String, _>("org_name"),
        "people": people, "answer": r.get::<Option<String>, _>("answer"),
    }))
}

#[derive(Deserialize)]
pub struct InviteAnswerIn {
    pub agree: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// Confirm or dispute. The link works once; the address is erased with the answer.
pub async fn answer_split_invite(s: &AppState, token: &str, i: InviteAnswerIn) -> Result<Value> {
    let note =
        opt_clean(i.note.as_deref(), 1000, "WORK_INVITE_NOTE_INVALID")?.filter(|n| !n.is_empty());
    if let Some(n) = &note {
        crate::text_policy::check_multiline(n)?;
    }
    let mut tx = s.pool.begin().await?;
    let r = invite_by_token(&mut tx, token).await?;
    if r.get::<Option<String>, _>("answer").is_some() {
        return Err(Error::PolicyGate("WORK_INVITE_ANSWERED"));
    }
    let answer = if i.agree { "confirmed" } else { "disputed" };
    let (work, contributor): (Uuid, Uuid) = (r.get("work_id"), r.get("contributor_id"));
    sqlx::query("UPDATE portal.pub_work_contributors SET confirm=$3 WHERE work_id=$1 AND id=$2")
        .bind(work)
        .bind(contributor)
        .bind(answer)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE portal.pub_split_invites SET answer=$2, note=$3, answered_at=now(), email_cipher=NULL, token_cipher=NULL, token_hash=$4,
                invite_status=CASE WHEN invite_status='PENDING' THEN 'SKIPPED' ELSE invite_status END
          WHERE id=$1",
    )
    .bind(r.get::<Uuid, _>("id"))
    .bind(answer)
    .bind(note.filter(|_| !i.agree))
    .bind(crate::domain::sha256_hex(auth::random_token()))
    .execute(&mut *tx)
    .await?;
    touch(&mut tx, work).await?;
    tx.commit().await?;
    Ok(json!({"answer": answer}))
}

/// Invites sent per round.
pub const INVITE_BATCH: i64 = 20;

fn invite_mail(
    origin: &str,
    token: &str,
    from: &str,
    title: &str,
    name: &str,
    share: &str,
) -> crate::email::Rendered {
    use crate::email::{escape_html, one_line};
    let (from, title, name) = (one_line(from), one_line(title), one_line(name));
    let url = format!("{origin}/split-confirm/{token}");
    let headline = format!("{from}에서 '{title}'의 지분 확인을 요청했어요");
    let body = format!(
        "'{title}'의 공동 작곡 참여자로 {name} 님의 창작 지분이 {share}로 적혀 있어요. \
         링크에서 맞는지 확인하거나, 다르면 의견을 남겨 주세요. AUDENIQ 계정이 없어도 돼요."
    );
    let footer =
        "이 링크는 받으신 분만 쓸 수 있고 한 번 답하면 끝나요. 모르는 내용이면 무시해 주세요.";
    let text = format!("{headline}\n\n{body}\n\n확인하기: {url}\n\n--\n{footer}\n");
    let html = format!(
        "<!doctype html><html lang=\"ko\"><body style=\"margin:0;padding:24px;background:#f5f6f8;font-family:'Pretendard Variable',Pretendard,-apple-system,'Apple SD Gothic Neo','Malgun Gothic',sans-serif\">\
<div style=\"max-width:520px;margin:0 auto;background:#ffffff;border-radius:16px;padding:28px\">\
<p style=\"margin:0 0 6px;font-size:12px;font-weight:700;letter-spacing:.08em;color:#3b63f6\">AUDENIQ</p>\
<h1 style=\"margin:0 0 14px;font-size:20px;line-height:1.4;color:#111827\">{headline}</h1>\
<p style=\"margin:0 0 20px;line-height:1.6;color:#374151\">{body}</p>\
<a href=\"{url}\" style=\"display:inline-block;padding:12px 20px;border-radius:10px;background:#3b63f6;color:#ffffff;text-decoration:none;font-weight:700\">지분 확인하기</a>\
<p style=\"margin:24px 0 0;font-size:12px;line-height:1.6;color:#6b7280\">{footer}</p>\
</div></body></html>",
        headline = escape_html(&headline),
        body = escape_html(&body),
        url = escape_html(&url),
    );
    crate::email::Rendered {
        subject: format!("[AUDENIQ] {headline}"),
        text,
        html,
    }
}

/// Mail waiting co-writer invites (the notification loop calls this).
pub async fn dispatch_invites(
    pool: &sqlx::PgPool,
    keys: &crate::payout_keys::KeyRing,
    mailer: &crate::email::Mailer,
) -> Result<usize> {
    use crate::email::{Attempt, attempt};
    use crate::payout_keys::Field;
    let rows = sqlx::query(
        "UPDATE portal.pub_split_invites i
            SET invite_attempts=i.invite_attempts+1, invite_next_at=now() + interval '10 minutes'
          WHERE i.id IN (SELECT id FROM portal.pub_split_invites WHERE invite_status='PENDING' AND invite_next_at <= now()
                          ORDER BY invite_next_at LIMIT $1 FOR UPDATE SKIP LOCKED)
          RETURNING i.id, i.invite_attempts, i.email_cipher, i.token_cipher, i.answer,
            (SELECT o.name FROM identity.orgs o WHERE o.id=i.org_id) AS org_name,
            (SELECT w.title FROM portal.pub_works w WHERE w.id=i.work_id) AS title,
            (SELECT c.name FROM portal.pub_work_contributors c WHERE c.work_id=i.work_id AND c.id=i.contributor_id) AS name,
            (SELECT c.share_bps FROM portal.pub_work_contributors c WHERE c.work_id=i.work_id AND c.id=i.contributor_id) AS share_bps",
    )
    .bind(INVITE_BATCH)
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("invite_attempts");
        let done = |status: &'static str| async move {
            sqlx::query("UPDATE portal.pub_split_invites SET invite_status=$2, token_cipher=CASE WHEN $2='SENT' THEN NULL ELSE token_cipher END WHERE id=$1 AND invite_attempts=$3 AND invite_status='PENDING'")
                .bind(id)
                .bind(status)
                .bind(attempts)
                .execute(pool)
                .await
                .map(|_| ())
        };
        let opened = |f: Field, col: &str| {
            row.get::<Option<Vec<u8>>, _>(col)
                .and_then(|c| keys.open_field(f, id, &c).ok())
        };
        let (Some(to), Some(token), None, Some(name)) = (
            opened(Field::PubInviteEmail, "email_cipher"),
            opened(Field::PubInviteToken, "token_cipher"),
            row.get::<Option<String>, _>("answer"),
            row.get::<Option<String>, _>("name"),
        ) else {
            done("SKIPPED").await?;
            continue;
        };
        let mail = invite_mail(
            &mailer.origin,
            &token,
            &row.get::<String, _>("org_name"),
            &row.get::<Option<String>, _>("title").unwrap_or_default(),
            &name,
            &format!(
                "{}%",
                pct(row.get::<Option<i32>, _>("share_bps").unwrap_or(0))
            ),
        );
        match attempt(mailer.send(Uuid::new_v4(), &to, &mail).await, attempts) {
            Attempt::Sent(_) => done("SENT").await?,
            Attempt::Failed(_) => done("FAILED").await?,
            Attempt::Retry(after) => {
                sqlx::query("UPDATE portal.pub_split_invites SET invite_next_at=now() + make_interval(secs => $2) WHERE id=$1 AND invite_attempts=$3")
                    .bind(id)
                    .bind(after)
                    .bind(attempts)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(rows.len())
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

#[allow(clippy::too_many_arguments)]
async fn attach(
    c: &mut PgConnection,
    org: Uuid,
    work: Uuid,
    user: Uuid,
    kind: &str,
    name: &str,
    size: Option<i64>,
    asset: Option<Uuid>,
    text: Option<&str>,
) -> Result<()> {
    sqlx::query("INSERT INTO portal.pub_work_attachments(id,org_id,work_id,kind,name,size_bytes,asset_id,text_body,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(work)
        .bind(kind)
        .bind(name)
        .bind(size)
        .bind(asset)
        .bind(text)
        .bind(user)
        .execute(&mut *c)
        .await?;
    Ok(())
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
    attach(
        &mut tx,
        org,
        id,
        a.user,
        &i.kind,
        &name,
        i.size,
        i.asset_id,
        text.as_deref(),
    )
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
        "SELECT t.id, t.title, t.asset_id, a.size_bytes, NULLIF(btrim(coalesce(t.lyrics,'')),'') AS lyrics,
                -- The track's row in the release form: audio file name and the instrumental flag.
                (SELECT d FROM jsonb_array_elements(CASE WHEN jsonb_typeof(r.draft->'draftTracks')='array' THEN r.draft->'draftTracks' ELSE '[]'::jsonb END) d
                  WHERE d->>'serverId' = t.id::text OR d->>'id' = t.id::text LIMIT 1) AS draft_track
           FROM catalog.tracks t JOIN catalog.releases r ON r.id=t.release_id
           LEFT JOIN catalog.assets a ON a.id=t.asset_id
          WHERE t.org_id=$1 AND t.release_id=$2 AND t.archived_at IS NULL
            AND NOT EXISTS(SELECT 1 FROM portal.pub_work_recordings x JOIN portal.pub_works w ON w.id=x.work_id
                            WHERE x.track_id=t.id AND w.archived_at IS NULL)
          ORDER BY t.disc_number, t.track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    // What the release form already has for the whole release — genre and language are not asked again.
    let (genre, language, ai_uses): (Option<String>, Option<String>, Option<Value>) = sqlx::query_as(
        "SELECT COALESCE(NULLIF(draft->>'genreCustom',''), NULLIF(draft->>'genre','')), NULLIF(draft->>'language',''),
                CASE WHEN (draft->'options'->>'ai')::boolean THEN COALESCE(draft->'options'->'aiUses','[]'::jsonb) END
           FROM catalog.releases WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    let genre: String = genre.unwrap_or_default().trim().chars().take(60).collect();
    let language = language
        .filter(|l| LANGUAGES.contains(&l.as_str()))
        .unwrap_or_else(|| "ko".into());
    // The release form's AI answer: made entirely by AI, or AI helped with the melody or lyrics (the song itself).
    // AI used only for vocals, mixing or the cover leaves the song as written.
    let said = |needle: &str| {
        ai_uses.as_ref().and_then(Value::as_array).is_some_and(|l| {
            l.iter()
                .any(|u| u.as_str().is_some_and(|u| u.contains(needle)))
        })
    };
    let ai_use = if said("전체를 AI") {
        "generated"
    } else if said("작곡") || said("작사") {
        "assisted"
    } else {
        "none"
    };
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
                    publisher_name: None,
                }
            })
            .collect();
        let draft: Option<Value> = t.get("draft_track");
        let instrumental = draft
            .as_ref()
            .and_then(|d| d["instrumental"].as_bool())
            .unwrap_or(false);
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO portal.pub_works(id,org_id,internal_code,title,registration,created_by,origin,genre,language,work_type,ai_use)
             VALUES($1,$2,'AQW-'||lpad(nextval('portal.pub_work_code_seq')::text,6,'0'),$3,$4,$5,'own_release',$6,$7,$8,$9)",
        )
        .bind(id)
        .bind(org)
        .bind(&title)
        .bind(if i.enroll { "preparing" } else { "none" })
        .bind(a.user)
        .bind(&genre)
        .bind(&language)
        .bind(if instrumental { "instrumental" } else { "original" })
        .bind(ai_use)
        .execute(&mut *tx)
        .await?;
        write_contributors(&mut tx, org, id, a.party, a.user, plan).await?;
        link_recording(&mut tx, org, id, release, track).await?;
        // The audio and lyrics sent with the release are the work's files too — the artist does not upload them again.
        if let Some(asset) = t.get::<Option<Uuid>, _>("asset_id") {
            let name: String = draft
                .as_ref()
                .and_then(|d| d["audioName"].as_str())
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(String::from)
                .unwrap_or_else(|| format!("{title} 음원"))
                .chars()
                .take(200)
                .collect();
            attach(
                &mut tx,
                org,
                id,
                a.user,
                "demo",
                &name,
                t.get("size_bytes"),
                Some(asset),
                None,
            )
            .await?;
        }
        if let Some(text) = t
            .get::<Option<String>, _>("lyrics")
            .filter(|_| !instrumental)
            && text.chars().count() <= 200_000
            && crate::text_policy::check_multiline(&text).is_ok()
        {
            let name: String = format!("{title} 가사 (배급 신청서)")
                .chars()
                .take(200)
                .collect();
            attach(
                &mut tx,
                org,
                id,
                a.user,
                "lyrics",
                &name,
                None,
                None,
                Some(&text),
            )
            .await?;
        }
        created += 1;
    }
    audit(&mut tx, a, org, release, "publishing.works_prepared").await?;
    tx.commit().await?;
    Ok(json!({"created": created, "linked": linked, "skipped": skipped}))
}

// ---------------------------------------------------------------------------
// Applications
// ---------------------------------------------------------------------------

/// What happened to an application, from the audit trail (no copy kept).
const HISTORY: &str = "COALESCE((SELECT jsonb_agg(jsonb_build_object('at',e.occurred_at,'text',
      CASE e.action WHEN 'publishing.application_submitted' THEN '제출 완료' WHEN 'publishing.application_claimed' THEN '심사 시작'
        WHEN 'publishing.application_request_correction' THEN '보완 요청' WHEN 'publishing.fix_resubmitted' THEN '보완 제출'
        WHEN 'publishing.application_approve' THEN '승인' WHEN 'publishing.application_reject' THEN '반려'
        WHEN 'publishing.application_register' THEN '관리 단체 등록 완료' WHEN 'publishing.application_withdrawn' THEN '신청 철회' END)
      ORDER BY e.occurred_at, e.id)
    FROM operations.audit_events e WHERE e.resource_id=a.id AND e.action LIKE 'publishing.%'),'[]')";

async fn applications_json(
    c: &mut PgConnection,
    org: Option<Uuid>,
    ids: Option<Vec<Uuid>>,
    status: Option<&str>,
) -> Result<Vec<Value>> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT a.id, a.org_id, o.name AS org_name, ra.application_no, a.status, a.territories, a.rights, a.submitted_at, a.fix_reason, a.reply_note,
                ra.server_hash, ra.signer_name,
                (SELECT c.id FROM portal.pub_contracts c WHERE c.application_id=a.id) AS contract_id,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('id',w.id,'title',w.title) ORDER BY w.title)
                            FROM portal.pub_application_works aw JOIN portal.pub_works w ON w.id=aw.work_id WHERE aw.application_id=a.id),'[]') AS works,
                {HISTORY} AS history
           FROM portal.pub_applications a JOIN identity.orgs o ON o.id=a.org_id JOIN portal.release_applications ra ON ra.id=a.record_id
          WHERE ($1::uuid IS NULL OR a.org_id=$1) AND ($2::uuid[] IS NULL OR a.id = ANY($2)) AND ($3::text IS NULL OR a.status=$3)
          ORDER BY a.submitted_at DESC, a.id
          LIMIT 200"
    )))
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
                "number": r.get::<String, _>("application_no"), "status": r.get::<String, _>("status"),
                "workIds": list.iter().map(|w| w["id"].clone()).collect::<Vec<_>>(),
                "workTitles": list.iter().map(|w| w["title"].clone()).collect::<Vec<_>>(),
                "territories": r.get::<Vec<String>, _>("territories"), "rights": r.get::<Vec<String>, _>("rights"),
                "submittedAt": ts(r.get("submitted_at")), "contractId": r.get::<Option<Uuid>, _>("contract_id"),
                "signerName": r.get::<String, _>("signer_name"),
                "fixReason": r.get::<Option<String>, _>("fix_reason"), "replyNote": r.get::<Option<String>, _>("reply_note"),
                // The confirmation code of the signed original, as on 배급 신청서.
                "documentCode": r.get::<Option<String>, _>("server_hash"),
                "history": r.get::<Value, _>("history"),
            })
        })
        .collect())
}

pub async fn list_applications(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, a, org).await?;
    Ok(json!({"items": applications_json(&mut c, Some(org), None, None).await?}))
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

/// The same number format as 배급 신청서 (`AUD-20261030-K7Q2MX`), made on the
/// server since the publishing screen does not number its own.
fn application_no() -> Result<String> {
    document_no("AUD")
}

fn document_no(prefix: &str) -> Result<String> {
    use rand::{TryRng, rngs::SysRng};
    const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; 6];
    SysRng
        .try_fill_bytes(&mut raw)
        .map_err(|_| Error::Internal)?;
    let day = (Utc::now() + chrono::Duration::hours(9)).format("%Y%m%d");
    let tail: String = raw
        .iter()
        .map(|b| ALPHABET[usize::from(b % 32)] as char)
        .collect();
    Ok(format!("{prefix}-{day}-{tail}"))
}

/// Submit an application (신청서 접수). What was signed is recorded like a
/// release application — `portal.release_applications` with form
/// `PUBLISHING`, the server's own copy and hash, the signer's verified
/// identity — and sealed. The same idempotency key returns the application
/// already made instead of a second one.
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
    let agreed = dedup(&i.agreed);
    if !AGREEMENTS.iter().all(|k| agreed.iter().any(|x| x == k))
        || agreed.iter().any(|x| !AGREEMENTS.contains(&x.as_str()))
    {
        return Err(Error::InvalidCode("APPLICATION_AGREEMENT_REQUIRED"));
    }
    let signer = clean(&i.signature.name, 120, "APPLICATION_SIGNATURE_INVALID")?;
    if signer.is_empty() || i.signature.method != "drawn" {
        return Err(Error::InvalidCode("APPLICATION_SIGNATURE_INVALID"));
    }
    // The same signature check (and AV scan) as 배급 신청서.
    let sig =
        crate::upload_safety::signature(s, i.signature.data.as_deref().unwrap_or_default()).await?;
    let key = i.idempotency_key.trim().to_string();
    if !(8..=120).contains(&key.len()) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
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
    // Administration is for released songs (own or someone else's); a song not released yet goes to Open A&R.
    let unreleased: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.pub_works w WHERE w.id = ANY($1) AND w.origin='unreleased'
                        AND NOT EXISTS(SELECT 1 FROM portal.pub_work_recordings r WHERE r.work_id=w.id))",
    )
    .bind(&works)
    .fetch_one(&mut *tx)
    .await?;
    if unreleased {
        return Err(Error::PolicyGate("WORK_NOT_RELEASED"));
    }
    // Societies register what a person wrote: a song made entirely by AI is not administered.
    let generated: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.pub_works WHERE id = ANY($1) AND ai_use='generated')",
    )
    .bind(&works)
    .fetch_one(&mut *tx)
    .await?;
    if generated {
        return Err(Error::PolicyGate("WORK_NOT_ELIGIBLE"));
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
    // The server's own copy of what was signed: every work as it stands now.
    let no = application_no()?;
    let submitted = Utc::now().to_rfc3339();
    let snapshot = works_json(&mut tx, org, a.party, &role, Filter::Ids(works.clone())).await?;
    let content = json!({
        "application_no": no, "form": "PUBLISHING", "terms_version": POLICY_VERSION,
        "works": snapshot, "territories": territories, "rights": rights,
        "signer": {"name": signer, "role": "저작권자", "agreements": agreed, "client_submitted_at": submitted,
                   "signature_sha256": crate::domain::sha256_hex(&sig)},
    });
    let server_hash = crate::domain::sha256_json(&content);
    let identity = crate::consents::signer_identity(&mut tx, a.user).await?;
    let record = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.release_applications(id,org_id,release_id,application_no,form,content_hash,signer_name,signer_role,agreements,signature,client_submitted_at,submitted_by,content,server_hash,signer_identity)
         VALUES($1,$2,NULL,$3,'PUBLISHING',$4,$5,'저작권자',$6,$7,$8,$9,$10,$4,$11)",
    )
    .bind(record)
    .bind(org)
    .bind(&no)
    .bind(&server_hash)
    .bind(&signer)
    .bind(&agreed)
    .bind(&sig)
    .bind(&submitted)
    .bind(a.user)
    .bind(&content)
    .bind(&identity)
    .execute(&mut *tx)
    .await?;
    crate::seal::seal(
        &mut tx,
        &s.payout_keys,
        crate::seal::Kind::Application,
        record,
        "SUBMITTED",
    )
    .await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.pub_applications(id,org_id,record_id,territories,rights,idempotency_key,submitted_by) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id)
        .bind(org)
        .bind(record)
        .bind(&territories)
        .bind(&rights)
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

/// Answer a fix request: the work goes back to review with the note. The
/// documents staff asked for are submitted on 권리·보완 서류 (the same
/// upload as release proofs); a work waits until none is still due.
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
    let due: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.documents WHERE org_id=$1 AND work_id=$2 AND kind='RIGHTS_PROOF' AND status IN ('AWAITING_DOCUMENTS','NEEDS'))",
    )
    .bind(org)
    .bind(work)
    .fetch_one(&mut *tx)
    .await?;
    if due {
        return Err(Error::PolicyGate("WORK_DOCUMENTS_DUE"));
    }
    let shares: Vec<ShareRow> = sqlx::query("SELECT name, roles, share_bps, publisher_share_bps, confirm FROM portal.pub_work_contributors WHERE work_id=$1")
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
    let app: Option<Uuid> = sqlx::query_scalar(
        "SELECT x.id FROM portal.pub_application_works aw JOIN portal.pub_applications x ON x.id=aw.application_id
          WHERE aw.work_id=$1 AND x.status='needs_fix' ORDER BY x.submitted_at DESC LIMIT 1",
    )
    .bind(work)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(app) = app {
        // Back to review once none of its works still waits for a fix.
        sqlx::query(
            "UPDATE portal.pub_applications SET reply_note=NULLIF($2,''),
                    status=CASE WHEN EXISTS(SELECT 1 FROM portal.pub_application_works aw JOIN portal.pub_works w ON w.id=aw.work_id
                                             WHERE aw.application_id=$1 AND w.registration='needs_fix') THEN status ELSE 'in_review' END
              WHERE id=$1",
        )
        .bind(app)
        .bind(&note)
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, a, org, app, "publishing.fix_resubmitted").await?;
    }
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
    // An approved application's contract waiting for the artist's signature (계약서).
    for (cid, titles) in sqlx::query_as::<_, (Uuid, Vec<String>)>(
        "SELECT c.id, COALESCE((SELECT array_agg(w.title ORDER BY w.title) FROM portal.pub_application_works aw
                                  JOIN portal.pub_works w ON w.id=aw.work_id WHERE aw.application_id=c.application_id),'{}')
           FROM portal.pub_contracts c WHERE c.org_id=$1 AND c.status='pending_sign' ORDER BY c.created_at",
    )
    .bind(org)
    .fetch_all(&mut *c)
    .await?
    {
        tasks.push(json!({"id": format!("sign-{cid}"), "kind": "sign", "title": "저작권 관리 계약서 서명",
            "sub": titles.join(", "), "to": format!("/contracts/publishing/{cid}")}));
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

/// The same shape as a release decision (`/api/staff/releases/{id}/decision`).
#[derive(Deserialize)]
pub struct DecisionIn {
    /// APPROVE · REQUEST_CORRECTION · REJECT · REGISTER (society registration done)
    pub action: String,
    #[serde(default)]
    pub reason: String,
    /// REQUEST_CORRECTION: only these works (default: every work of the application).
    #[serde(default)]
    pub work_ids: Vec<Uuid>,
    #[serde(default)]
    pub fields: Vec<String>,
    /// REQUEST_CORRECTION: documents to submit on 권리·보완 서류, one row per work.
    #[serde(default)]
    pub documents: Vec<String>,
    /// REGISTER: the collecting society the works were registered with.
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

async fn staff_audit(
    c: &mut PgConnection,
    st: &staff::Staff,
    org: Uuid,
    id: Uuid,
    action: &str,
) -> Result<()> {
    operations::audit(
        c,
        Some(st.actor.user),
        Some(org),
        Some(id),
        action,
        "STAFF",
        st.actor.request,
    )
    .await
}

/// Take an application into review (as `/api/staff/releases/{id}/claim`).
pub async fn staff_claim(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Review)?;
    let mut tx = s.pool.begin().await?;
    let (org, status, _) = lock_application(&mut tx, None, id).await?;
    if status != "submitted" {
        return Err(Error::PolicyGate("APPLICATION_STATE"));
    }
    sqlx::query("UPDATE portal.pub_applications SET status='in_review', claimed_by=$2 WHERE id=$1")
        .bind(id)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
    staff_audit(&mut tx, &st, org, id, "publishing.application_claimed").await?;
    let out = applications_json(&mut tx, None, Some(vec![id]), None)
        .await?
        .pop()
        .ok_or(Error::Internal)?;
    tx.commit().await?;
    Ok(out)
}

pub async fn staff_decide(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionIn) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Review)?;
    let reason = clean(&i.reason, 2000, "APPLICATION_REASON_INVALID")?;
    let mut tx = s.pool.begin().await?;
    let (org, status, works) = lock_application(&mut tx, None, id).await?;
    let open = ["submitted", "in_review"].contains(&status.as_str());
    match i.action.as_str() {
        "APPROVE" if open => {
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
            // The contract is made now, with the confirmed terms; the artist signs it on 계약서.
            create_contract(&mut tx, org, id).await?;
        }
        "REQUEST_CORRECTION" if open => {
            // What to fix is picked (fields · documents); `reason` is the reviewer's own comment and may be
            // left out. Studio shows the comment only when it was written, like a release REVIEW_NOTE.
            if reason.is_empty() && i.fields.is_empty() && i.documents.is_empty() {
                return Err(Error::InvalidCode("APPLICATION_REASON_REQUIRED"));
            }
            if i.fields.iter().any(|f| fix_field_label(f).is_none()) {
                return Err(Error::Invalid);
            }
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
            // `reason` stays the one-line summary every screen and log already reads; `note` is the comment alone.
            let summary = fix_summary(&i.fields, &documents, &reason);
            let note = (!reason.is_empty()).then_some(reason.as_str());
            let fix = json!({"reason": summary, "note": note, "fields": i.fields, "documents": documents, "requestedAt": Utc::now().to_rfc3339()});
            let reason = summary;
            sqlx::query("UPDATE portal.pub_applications SET status='needs_fix', fix_reason=$2, reply_note=NULL WHERE id=$1")
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
            // Each document waits on 권리·보완 서류, like a proof asked for a release.
            for w in &target {
                let title: String =
                    sqlx::query_scalar("SELECT title FROM portal.pub_works WHERE id=$1")
                        .bind(w)
                        .fetch_one(&mut *tx)
                        .await?;
                for d in &documents {
                    sqlx::query(
                        "INSERT INTO portal.documents(id,org_id,release_id,work_id,kind,title,review_note,status)
                         VALUES($1,$2,NULL,$3,'RIGHTS_PROOF',$4,$5,'AWAITING_DOCUMENTS')",
                    )
                    .bind(Uuid::new_v4())
                    .bind(org)
                    .bind(w)
                    .bind(format!("{} · {d}", title.chars().take(80).collect::<String>()))
                    .bind(note.unwrap_or(&reason).chars().take(1000).collect::<String>())
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        "REJECT" if open || status == "needs_fix" => {
            if reason.is_empty() {
                return Err(Error::InvalidCode("APPLICATION_REASON_REQUIRED"));
            }
            sqlx::query("UPDATE portal.pub_applications SET status='rejected', fix_reason=$2, decided_by=$3, decided_at=now() WHERE id=$1")
                .bind(id)
                .bind(&reason)
                .bind(st.actor.user)
                .execute(&mut *tx)
                .await?;
            set_registration(&mut tx, org, &works, "rejected", None, Some(&reason)).await?;
        }
        "REGISTER" if status == "approved" => {
            let signed: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM portal.pub_contracts WHERE application_id=$1 AND status <> 'pending_sign')",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if !signed {
                return Err(Error::PolicyGate("CONTRACT_NOT_SIGNED"));
            }
            let society = opt_clean(i.society.as_deref(), 60, "WORK_SOCIETY_INVALID")?
                .ok_or(Error::Invalid)?;
            for w in &works {
                let n = sqlx::query(
                    "UPDATE portal.pub_works SET registration='registered', row_version=row_version+1, updated_at=now()
                      WHERE id=$1 AND registration IN ('ready','submitted','external_review')",
                )
                .bind(w)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if n == 1 {
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
            }
        }
        "APPROVE" | "REQUEST_CORRECTION" | "REJECT" | "REGISTER" => {
            return Err(Error::PolicyGate("APPLICATION_STATE"));
        }
        _ => return Err(Error::Invalid),
    }
    staff_audit(
        &mut tx,
        &st,
        org,
        id,
        &format!("publishing.application_{}", i.action.to_lowercase()),
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
// Contracts (저작권 관리 계약서, migration 0168)
// ---------------------------------------------------------------------------

/// What the artist ticks before signing the contract; every box is required.
const CONTRACT_CONFIRMATIONS: [(&str, &str); 3] = [
    ("read", "저작권 관리 계약서의 내용을 모두 읽고 이해했어요."),
    (
        "rights",
        "계약한 곡에 대해 신고한 지분만큼 권리를 가지고 있어요.",
    ),
    (
        "exclusive",
        "계약 기간 동안 같은 권리를 다른 퍼블리셔에게 맡기지 않아요.",
    ),
];

fn territory_label(code: &str) -> &str {
    TERRITORIES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(code, |(_, l)| l)
}
fn right_label(key: &str) -> &str {
    RIGHTS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or(key, |(_, l, _)| l)
}
fn term_label(months: i32) -> String {
    if months % 12 == 0 {
        format!("{}년", months / 12)
    } else {
        format!("{months}개월")
    }
}

/// The contract text as it is made at approval (one paragraph per line). Kept with the contract so a later
/// policy change does not alter what was offered or signed.
fn contract_body(
    works: &[(String, String)],
    territories: &[String],
    rights: &[String],
    commission_bps: i32,
    term_months: i32,
) -> String {
    let list = works
        .iter()
        .map(|(t, code)| format!("‘{t}’({code})"))
        .collect::<Vec<_>>()
        .join(", ");
    let terr = territories
        .iter()
        .map(|t| territory_label(t))
        .collect::<Vec<_>>()
        .join(", ");
    let rts = rights
        .iter()
        .map(|r| right_label(r))
        .collect::<Vec<_>>()
        .join(", ");
    let renew = RENEWAL_OPTIONS
        .iter()
        .map(|m| term_label(*m))
        .collect::<Vec<_>>()
        .join(", ");
    let rate = f64::from(commission_bps) / 100.0;
    [
        "제1조 (목적) 이 계약은 작가가 아래 곡의 저작권 관리를 주식회사 AUDENIQ(이하 ‘회사’)에 맡기고, 회사가 이를 관리하는 데 필요한 사항을 정한다.".to_string(),
        format!("제2조 (대상 곡) {list}. 작가는 각 곡에 신고한 지분만큼의 권리를 가진다."),
        format!("제3조 (관리 범위) 회사는 {terr}에서 {rts}을(를) 관리한다. 관리 단체 등록, 사용료 징수, 이용 허락 협의를 포함한다."),
        format!("제4조 (수수료) 회사는 실제 징수한 사용료의 {rate}%를 관리 수수료로 공제하고 나머지를 작가에게 지급한다. 예상 금액은 지급 대상이 아니다."),
        format!("제5조 (기간) 계약 기간은 작가가 서명한 날부터 {}로 하며, 작가가 요청하면 {renew} 단위로 연장한다.", term_label(term_months)),
        "제6조 (공동 저작) 공동 참여자의 지분은 각 참여자의 확인을 거쳐 반영한다. 확인되지 않은 지분은 확인될 때까지 사용료 지급을 보류할 수 있다.".to_string(),
        "제7조 (종료) 어느 한쪽이 종료를 요청하면 관리 단체 절차에 따라 관리를 마친다. 종료 전 발생한 사용료는 종료 후에도 정산한다.".to_string(),
        "제8조 (기존 계약) 작가는 다른 퍼블리셔와의 계약을 신고해야 하며, 충돌이 있으면 회사가 검토한 뒤 관리 범위를 조정할 수 있다.".to_string(),
        "제9조 (전자계약) 이 계약은 전자문서로 작성하고 전자서명으로 체결한다. 체결일은 작가가 서명한 날이며, 서명·확인 항목·문서 확인 코드는 함께 보관한다.".to_string(),
    ]
    .join("\n")
}

/// Make the contract for an approved application (once; approving again finds it).
async fn create_contract(c: &mut PgConnection, org: Uuid, application: Uuid) -> Result<Uuid> {
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM portal.pub_contracts WHERE application_id=$1")
            .bind(application)
            .fetch_optional(&mut *c)
            .await?
    {
        return Ok(id);
    }
    let (territories, rights): (Vec<String>, Vec<String>) =
        sqlx::query_as("SELECT territories, rights FROM portal.pub_applications WHERE id=$1")
            .bind(application)
            .fetch_one(&mut *c)
            .await?;
    let works: Vec<(String, String)> = sqlx::query_as(
        "SELECT w.title, COALESCE(w.iswc, w.internal_code) FROM portal.pub_application_works aw
           JOIN portal.pub_works w ON w.id=aw.work_id WHERE aw.application_id=$1 ORDER BY w.title",
    )
    .bind(application)
    .fetch_all(&mut *c)
    .await?;
    let body = contract_body(
        &works,
        &territories,
        &rights,
        COMMISSION_BPS,
        INITIAL_TERM_MONTHS,
    );
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.pub_contracts(id,org_id,application_id,contract_no,commission_bps,term_months,territories,rights,body)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(org)
    .bind(application)
    .bind(document_no("AQP")?)
    .bind(COMMISSION_BPS)
    .bind(INITIAL_TERM_MONTHS)
    .bind(&territories)
    .bind(&rights)
    .bind(&body)
    .execute(&mut *c)
    .await?;
    Ok(id)
}

/// Contracts as the screens read them (`PublishingContract` in api/publishing.ts), with the releases their
/// works are on so 계약서 can show each one in that release's bundle.
async fn contracts_json(
    c: &mut PgConnection,
    org: Uuid,
    ids: Option<Vec<Uuid>>,
) -> Result<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT c.id, c.contract_no, c.status, c.commission_bps, c.term_months, c.territories, c.rights, c.body,
                c.signer_name, c.signature, c.confirmations, c.content_hash, c.signed_at, c.start_date, c.end_date,
                c.renewal_months, c.renewal_requested_at, c.termination_requested_at, c.row_version, c.created_at,
                a.id AS application_id, ra.application_no,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('id',w.id,'title',w.title) ORDER BY w.title)
                            FROM portal.pub_application_works aw JOIN portal.pub_works w ON w.id=aw.work_id
                           WHERE aw.application_id=a.id),'[]') AS works,
                COALESCE((SELECT array_agg(DISTINCT x.release_id) FROM portal.pub_application_works aw
                            JOIN portal.pub_work_recordings x ON x.work_id=aw.work_id WHERE aw.application_id=a.id),'{}') AS releases
           FROM portal.pub_contracts c
           JOIN portal.pub_applications a ON a.id=c.application_id
           JOIN portal.release_applications ra ON ra.id=a.record_id
          WHERE c.org_id=$1 AND ($2::uuid[] IS NULL OR c.id = ANY($2))
          ORDER BY c.created_at DESC, c.id
          LIMIT 200",
    )
    .bind(org)
    .bind(ids)
    .fetch_all(&mut *c)
    .await?;
    let date = |d: Option<chrono::NaiveDate>| {
        d.map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    };
    Ok(rows
        .into_iter()
        .map(|r| {
            let renewal = r.get::<Option<i32>, _>("renewal_months").map(|m| {
                json!({"months": m, "requestedAt": r.get::<Option<DateTime<Utc>>, _>("renewal_requested_at").map(ts)})
            });
            json!({
                "id": r.get::<Uuid, _>("id"), "number": r.get::<String, _>("contract_no"), "status": r.get::<String, _>("status"),
                "startDate": date(r.get("start_date")), "endDate": date(r.get("end_date")),
                "commissionRate": f64::from(r.get::<i32, _>("commission_bps")) / 100.0, "termMonths": r.get::<i32, _>("term_months"),
                "works": r.get::<Value, _>("works"), "territories": r.get::<Vec<String>, _>("territories"),
                "rights": r.get::<Vec<String>, _>("rights"), "renewal": renewal,
                "terminationRequestedAt": r.get::<Option<DateTime<Utc>>, _>("termination_requested_at").map(ts),
                "signedAt": r.get::<Option<DateTime<Utc>>, _>("signed_at").map(ts),
                "body": r.get::<String, _>("body"),
                "confirmations": CONTRACT_CONFIRMATIONS.iter().map(|(k, t)| json!({"id": k, "text": t})).collect::<Vec<_>>(),
                "ticked": r.get::<Option<Value>, _>("confirmations"),
                "signerName": r.get::<Option<String>, _>("signer_name"),
                "signature": r.get::<Option<String>, _>("signature"),
                "contentHash": r.get::<Option<String>, _>("content_hash"),
                "applicationId": r.get::<Uuid, _>("application_id"),
                "applicationNumber": r.get::<String, _>("application_no"),
                "releaseIds": r.get::<Vec<Uuid>, _>("releases"),
                "createdAt": ts(r.get("created_at")),
                "rowVersion": r.get::<i64, _>("row_version"),
            })
        })
        .collect())
}

pub async fn list_contracts(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    auth::membership_read(&mut c, a, org).await?;
    Ok(json!({"items": contracts_json(&mut c, org, None).await?}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractSignIn {
    pub signer_name: String,
    /// The drawn signature (data URL), checked like 배급 계약서.
    pub signature: String,
    #[serde(default)]
    pub confirmations: Vec<String>,
    pub row_version: i64,
}

async fn lock_contract(c: &mut PgConnection, org: Uuid, id: Uuid) -> Result<(String, i64, i32)> {
    sqlx::query_as("SELECT status, row_version, term_months FROM portal.pub_contracts WHERE org_id=$1 AND id=$2 FOR UPDATE")
        .bind(org)
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)
}

async fn contract_out(c: &mut PgConnection, org: Uuid, id: Uuid) -> Result<Value> {
    contracts_json(c, org, Some(vec![id]))
        .await?
        .pop()
        .ok_or(Error::Internal)
}

/// Sign the contract (계약 체결): every confirmation ticked, the drawn signature checked like 배급 계약서,
/// a hash over body + terms + ticks + signature kept with it. The term starts on the day of signing (KST).
pub async fn sign_contract(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: ContractSignIn,
) -> Result<Value> {
    let name = clean(&i.signer_name, 120, "CONTRACT_SIGNER_INVALID")?;
    if name.is_empty() {
        return Err(Error::InvalidCode("CONTRACT_SIGNER_INVALID"));
    }
    if !CONTRACT_CONFIRMATIONS
        .iter()
        .all(|(k, _)| i.confirmations.iter().any(|x| x == k))
        || i.confirmations
            .iter()
            .any(|x| !CONTRACT_CONFIRMATIONS.iter().any(|(k, _)| k == x))
    {
        return Err(Error::PolicyGate("AGREEMENT_CONFIRMATION_REQUIRED"));
    }
    let sig = crate::upload_safety::signature(s, &i.signature).await?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let (status, rv, months) = lock_contract(&mut tx, org, id).await?;
    if rv != i.row_version {
        return Err(Error::Conflict);
    }
    if status != "pending_sign" {
        return Err(Error::PolicyGate("CONTRACT_ALREADY_SIGNED"));
    }
    let (body, commission, territories, rights): (String, i32, Vec<String>, Vec<String>) = sqlx::query_as(
        "SELECT body, commission_bps, territories, rights FROM portal.pub_contracts WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let ticks = json!({"items": CONTRACT_CONFIRMATIONS.iter().map(|(k, t)| json!({"id": k, "text": t, "checked": true})).collect::<Vec<_>>()});
    let hash = crate::domain::sha256_json(&json!({
        "body": body,
        "terms": {"commission_bps": commission, "term_months": months, "territories": territories, "rights": rights},
        "ticks": ticks["items"], "signature_sha256": crate::domain::sha256_hex(&sig),
    }));
    let start = (Utc::now() + chrono::Duration::hours(9)).date_naive();
    let end = start
        .checked_add_months(chrono::Months::new(
            u32::try_from(months).map_err(|_| Error::Internal)?,
        ))
        .ok_or(Error::Internal)?;
    sqlx::query(
        "UPDATE portal.pub_contracts SET status='active', signer_name=$2, signature=$3, confirmations=$4, content_hash=$5,
                signed_by=$6, signed_at=now(), start_date=$7, end_date=$8, row_version=row_version+1 WHERE id=$1",
    )
    .bind(id)
    .bind(&name)
    .bind(&sig)
    .bind(&ticks)
    .bind(&hash)
    .bind(a.user)
    .bind(start)
    .bind(end)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, a, org, id, "publishing.contract_signed").await?;
    let out = contract_out(&mut tx, org, id).await?;
    tx.commit().await?;
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewIn {
    pub months: i32,
}

/// Ask to extend a signed contract (staff apply the new term).
pub async fn renew_contract(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: RenewIn,
) -> Result<Value> {
    if !RENEWAL_OPTIONS.contains(&i.months) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let (status, _, _) = lock_contract(&mut tx, org, id).await?;
    if !["active", "expiring"].contains(&status.as_str()) {
        return Err(Error::PolicyGate("CONTRACT_STATE"));
    }
    sqlx::query("UPDATE portal.pub_contracts SET renewal_months=$2, renewal_requested_at=now(), row_version=row_version+1 WHERE id=$1")
        .bind(id)
        .bind(i.months)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, a, org, id, "publishing.contract_renewal_requested").await?;
    let out = contract_out(&mut tx, org, id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Ask to end a signed contract (staff follow the society procedure).
pub async fn request_termination(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let (status, _, _) = lock_contract(&mut tx, org, id).await?;
    if !["active", "expiring"].contains(&status.as_str()) {
        return Err(Error::PolicyGate("CONTRACT_STATE"));
    }
    sqlx::query("UPDATE portal.pub_contracts SET status='terminating', termination_requested_at=now(), row_version=row_version+1 WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        a,
        org,
        id,
        "publishing.contract_termination_requested",
    )
    .await?;
    let out = contract_out(&mut tx, org, id).await?;
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
async fn h_invite_contributor(
    State(s): State<AppState>,
    Path((org, id, cid)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    body: Option<Json<InviteIn>>,
) -> Result<Json<Value>> {
    let i = body.map(|Json(b)| b).unwrap_or_default();
    Ok(Json(
        invite_contributor(&s, &writer(&s, &h).await?, org, id, cid, i).await?,
    ))
}
async fn h_ar_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_ar(&s, &reader(&s, &h).await?, org).await?))
}
async fn h_ar_submit(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ArIn>,
) -> Result<Json<Value>> {
    Ok(Json(submit_ar(&s, &writer(&s, &h).await?, org, i).await?))
}
async fn h_ar_withdraw(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        withdraw_ar(&s, &writer(&s, &h).await?, org, id).await?,
    ))
}
async fn h_split_invite(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(split_invite(&s, &token).await?))
}
async fn h_split_answer(
    State(s): State<AppState>,
    Path(token): Path<String>,
    Json(i): Json<InviteAnswerIn>,
) -> Result<Json<Value>> {
    Ok(Json(answer_split_invite(&s, &token, i).await?))
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
async fn h_contracts(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_contracts(&s, &reader(&s, &h).await?, org).await?))
}
async fn h_contract_sign(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<ContractSignIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        sign_contract(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_contract_renew(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<RenewIn>,
) -> Result<Json<Value>> {
    Ok(Json(
        renew_contract(&s, &writer(&s, &h).await?, org, id, i).await?,
    ))
}
async fn h_contract_end(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        request_termination(&s, &writer(&s, &h).await?, org, id).await?,
    ))
}
/// Stage 2 lists (royalties) answer empty so the screens show
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
async fn h_staff_claim(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_claim(&s, &h, id).await?))
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
        .route("/api/orgs/{org}/publishing/contracts", get(h_contracts))
        .route(
            "/api/orgs/{org}/publishing/contracts/{id}/sign",
            post(h_contract_sign),
        )
        .route(
            "/api/orgs/{org}/publishing/contracts/{id}/renewal",
            post(h_contract_renew),
        )
        .route(
            "/api/orgs/{org}/publishing/contracts/{id}/termination",
            post(h_contract_end),
        )
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
        .route(
            "/api/orgs/{org}/works/{id}/rights/{contributor}/invite",
            post(h_invite_contributor),
        )
        .route(
            "/api/orgs/{org}/ar/submissions",
            get(h_ar_list).post(h_ar_submit),
        )
        .route(
            "/api/orgs/{org}/ar/submissions/{id}/withdraw",
            post(h_ar_withdraw),
        )
        // The co-writer's side — no account, the link is the secret (as /api/split-invites).
        .route("/api/pub-invites/{token}", get(h_split_invite))
        .route("/api/pub-invites/{token}/answer", post(h_split_answer))
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
            "/api/staff/publishing/applications/{id}/claim",
            post(h_staff_claim),
        )
        .route(
            "/api/staff/publishing/applications/{id}/decision",
            post(h_staff_decide),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_summary_names_the_fields_and_documents() {
        let f = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            fix_summary(
                &f(&["contributors.ipi", "contributors.split"]),
                &f(&["공동 작곡 합의서"]),
                "메모"
            ),
            "참여자 IPI 번호 · 참여자·지분 · 서류 공동 작곡 합의서 보완"
        );
        assert_eq!(
            fix_summary(&[], &[], "IPI를 적어 주세요."),
            "IPI를 적어 주세요."
        );
        assert!(fix_field_label("work.title").is_none());
    }

    #[test]
    fn recording_status_reads_like_the_release() {
        assert_eq!(
            release_chip("STAGE1_CORRECTION", false, Some("REVIEW")),
            "needs"
        );
        assert_eq!(release_chip("ON_HOLD_RIGHTS", false, None), "needs");
        assert_eq!(release_chip("STAGE2_REVIEW", false, None), "review");
        assert_eq!(
            release_chip("READY_FOR_DELIVERY", false, Some("REVIEW")),
            "review"
        );
        assert_eq!(
            release_chip("READY_FOR_DELIVERY", false, Some("APPROVED")),
            "scheduled"
        );
        assert_eq!(
            release_chip("READY_FOR_DELIVERY", true, Some("SIGNED")),
            "live"
        );
        assert_eq!(
            release_chip("READY_FOR_DELIVERY", true, Some("NEEDS")),
            "needs"
        );
        assert_eq!(
            release_chip("WITHDRAWN", false, Some("CANCELLED")),
            "cancelled"
        );
        assert_eq!(release_chip("WITHDRAWN", false, None), "rejected");
        assert_eq!(release_chip("SUPERSEDED", false, None), "closed");
        assert_eq!(release_chip("DRAFT", false, None), "draft");
    }

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
            publisher: None,
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
