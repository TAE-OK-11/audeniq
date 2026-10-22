//! Common entity layer: what a task link or a note can point at. Only
//! (kind, id) is stored in `work.*`; existence and the display label are read
//! from each entity's own table here, so the work module never copies a
//! release title, an artist name or a DSP name.
use crate::{
    api::AppState,
    error::{Error, Result},
    staff,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::collections::HashMap;
use uuid::Uuid;

pub const KINDS: &[&str] = &[
    "release",
    "org",
    "artist",
    "label",
    "dsp",
    "inquiry",
    "document",
    "payout",
    "settlement_report",
    "task",
    "relationship",
    "channel",
    "doc",
    "track",
    "approval",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ref {
    pub kind: String,
    pub id: String,
}

/// Validate the shape of a reference (kind known; id a UUID, or a D-code for DSPs).
pub fn parse(kind: &str, id: &str) -> Result<Ref> {
    super::one_of(kind, KINDS, "ENTITY_KIND_UNKNOWN")?;
    let id = id.trim();
    let ok = match kind {
        "dsp" => {
            id.len() <= 5
                && id.starts_with("D-")
                && id[2..].chars().all(|c| c.is_ascii_digit())
                && id.len() > 2
        }
        _ => Uuid::parse_str(id).is_ok(),
    };
    if !ok {
        return Err(Error::InvalidCode("ENTITY_ID_INVALID"));
    }
    Ok(Ref {
        kind: kind.to_string(),
        id: id.to_string(),
    })
}

fn uuids(ids: &[&str]) -> Vec<Uuid> {
    ids.iter().filter_map(|i| Uuid::parse_str(i).ok()).collect()
}

/// Rows of (id, label, sub) for the given ids of one kind.
async fn fetch(
    c: &mut PgConnection,
    kind: &str,
    ids: &[&str],
) -> Result<Vec<(String, String, String)>> {
    let rows = match kind {
        "release" => {
            sqlx::query(
                "SELECT r.id::text AS id, r.title AS label, o.name AS sub FROM catalog.releases r
                   JOIN identity.orgs o ON o.id=r.org_id WHERE r.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "org" => {
            sqlx::query("SELECT o.id::text AS id, o.name AS label, o.kind AS sub FROM identity.orgs o WHERE o.id = ANY($1)")
                .bind(uuids(ids))
                .fetch_all(&mut *c)
                .await?
        }
        "artist" => {
            sqlx::query(
                "SELECT a.id::text AS id, a.name AS label, o.name AS sub FROM catalog.artists a
                   JOIN identity.orgs o ON o.id=a.org_id WHERE a.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "label" => {
            sqlx::query(
                "SELECT l.id::text AS id, l.name AS label, o.name AS sub FROM catalog.labels l
                   JOIN identity.orgs o ON o.id=l.org_id WHERE l.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "dsp" => {
            sqlx::query("SELECT d.code AS id, d.name AS label, d.code AS sub FROM distribution.dsp_registry d WHERE d.code = ANY($1)")
                .bind(ids.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .fetch_all(&mut *c)
                .await?
        }
        "inquiry" => {
            sqlx::query(
                "SELECT q.id::text AS id, q.subject AS label, o.name AS sub FROM portal.inquiries q
                   JOIN identity.orgs o ON o.id=q.org_id WHERE q.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "document" => {
            sqlx::query(
                "SELECT d.id::text AS id, d.title AS label, d.kind || ' · ' || o.name AS sub FROM portal.documents d
                   JOIN identity.orgs o ON o.id=d.org_id WHERE d.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "payout" => {
            sqlx::query(
                "SELECT p.id::text AS id, o.name || ' · ' || p.amount::text || ' ' || p.currency AS label, p.status AS sub
                   FROM portal.payout_requests p JOIN identity.orgs o ON o.id=p.org_id WHERE p.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "settlement_report" => {
            sqlx::query(
                "SELECT r.id::text AS id, r.dsp_id || ' ' || r.period_start::text || '~' || r.period_end::text AS label,
                        o.name || ' · ' || r.status AS sub
                   FROM finance.royalty_reports r JOIN identity.orgs o ON o.id=r.org_id WHERE r.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        // A restricted document never shows its title through a link or embed:
        // only its readers see it (docs.rs checks the level).
        "doc" => {
            sqlx::query(
                "SELECT d.id::text AS id,
                        CASE WHEN d.staff_access IS NULL THEN '제한된 문서' ELSE COALESCE(NULLIF(d.title,''),'제목 없음') END AS label,
                        d.status AS sub
                   FROM work.docs d WHERE d.id = ANY($1) AND d.deleted_at IS NULL",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "track" => {
            sqlx::query(
                "SELECT t.id::text AS id, t.title AS label, r.title AS sub FROM catalog.tracks t
                   JOIN catalog.releases r ON r.id=t.release_id WHERE t.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "approval" => {
            sqlx::query(
                "SELECT a.id::text AS id, '2차 승인 · ' || r.title AS label, a.status AS sub FROM rights.staff_approvals a
                   JOIN catalog.releases r ON r.id=a.release_id WHERE a.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "channel" => {
            sqlx::query(
                "SELECT c.id::text AS id, '#' || c.name AS label, c.topic AS sub FROM work.channels c WHERE c.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "relationship" => {
            sqlx::query(concat!(
                "SELECT r.id::text AS id, COALESCE(",
                super::crm::source_name!(),
                ", '?') AS label, r.kind || ' · ' || r.status AS sub FROM work.relationships r WHERE r.id = ANY($1)"
            ))
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        "task" => {
            sqlx::query(
                "SELECT t.id::text AS id, 'WRK-' || t.seq || ' ' || t.title AS label, t.status AS sub FROM work.tasks t WHERE t.id = ANY($1)",
            )
            .bind(uuids(ids))
            .fetch_all(&mut *c)
            .await?
        }
        _ => return Err(Error::InvalidCode("ENTITY_KIND_UNKNOWN")),
    };
    Ok(rows
        .iter()
        .map(|r| (r.get("id"), r.get("label"), r.get("sub")))
        .collect())
}

/// Labels for many references at once (one query per kind). A reference
/// whose source row is gone resolves to `missing: true`.
pub async fn resolve(c: &mut PgConnection, refs: &[Ref]) -> Result<HashMap<Ref, Value>> {
    let mut by_kind: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in refs {
        by_kind
            .entry(r.kind.as_str())
            .or_default()
            .push(r.id.as_str());
    }
    let mut out = HashMap::new();
    for (kind, ids) in by_kind {
        for (id, label, sub) in fetch(c, kind, &ids).await? {
            out.insert(
                Ref {
                    kind: kind.to_string(),
                    id: id.clone(),
                },
                json!({"kind": kind, "id": id, "label": label, "sub": sub}),
            );
        }
    }
    for r in refs {
        out.entry(r.clone()).or_insert_with(
            || json!({"kind": r.kind, "id": r.id, "label": null, "sub": null, "missing": true}),
        );
    }
    Ok(out)
}

/// The referenced entity must exist before anything links to it.
pub async fn require(c: &mut PgConnection, r: &Ref) -> Result<()> {
    if fetch(c, &r.kind, &[r.id.as_str()]).await?.is_empty() {
        return Err(Error::InvalidCode("ENTITY_NOT_FOUND"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    pub kind: String,
    #[serde(default)]
    pub q: String,
}

/// Find an entity to link (title / name contains `q`, or the exact id).
pub async fn search(s: &AppState, h: &HeaderMap, q: SearchQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    super::one_of(&q.kind, KINDS, "ENTITY_KIND_UNKNOWN")?;
    let term = super::line(&q.q, 80)?;
    let like = format!(
        "%{}%",
        term.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut c = s.pool.acquire().await?;
    let ids: Vec<String> = match q.kind.as_str() {
        "release" => sqlx::query_scalar("SELECT id::text FROM catalog.releases WHERE archived_at IS NULL AND (title ILIKE $1 OR id::text=$2) ORDER BY created_at DESC LIMIT 20"),
        "org" => sqlx::query_scalar("SELECT id::text FROM identity.orgs WHERE name ILIKE $1 OR id::text=$2 ORDER BY name LIMIT 20"),
        "artist" => sqlx::query_scalar("SELECT id::text FROM catalog.artists WHERE archived_at IS NULL AND (name ILIKE $1 OR id::text=$2) ORDER BY name LIMIT 20"),
        "label" => sqlx::query_scalar("SELECT id::text FROM catalog.labels WHERE archived_at IS NULL AND (name ILIKE $1 OR id::text=$2) ORDER BY name LIMIT 20"),
        "dsp" => sqlx::query_scalar("SELECT code FROM distribution.dsp_registry WHERE name ILIKE $1 OR code=$2 ORDER BY code LIMIT 50"),
        "inquiry" => sqlx::query_scalar("SELECT id::text FROM portal.inquiries WHERE subject ILIKE $1 OR id::text=$2 ORDER BY created_at DESC LIMIT 20"),
        "document" => sqlx::query_scalar("SELECT id::text FROM portal.documents WHERE title ILIKE $1 OR id::text=$2 ORDER BY updated_at DESC LIMIT 20"),
        "payout" => sqlx::query_scalar("SELECT p.id::text FROM portal.payout_requests p JOIN identity.orgs o ON o.id=p.org_id WHERE o.name ILIKE $1 OR p.id::text=$2 ORDER BY p.created_at DESC LIMIT 20"),
        "settlement_report" => sqlx::query_scalar("SELECT id::text FROM finance.royalty_reports WHERE dsp_id ILIKE $1 OR source_filename ILIKE $1 OR id::text=$2 ORDER BY received_at DESC LIMIT 20"),
        "doc" => sqlx::query_scalar("SELECT id::text FROM work.docs WHERE deleted_at IS NULL AND archived_at IS NULL AND (title ILIKE $1 OR id::text=$2) ORDER BY updated_at DESC LIMIT 60"),
        "track" => sqlx::query_scalar("SELECT id::text FROM catalog.tracks WHERE archived_at IS NULL AND (title ILIKE $1 OR isrc=upper($2) OR id::text=$2) ORDER BY title LIMIT 20"),
        "approval" => sqlx::query_scalar("SELECT a.id::text FROM rights.staff_approvals a JOIN catalog.releases r ON r.id=a.release_id WHERE r.title ILIKE $1 OR a.id::text=$2 ORDER BY a.created_at DESC LIMIT 20"),
        "channel" => sqlx::query_scalar("SELECT id::text FROM work.channels WHERE archived_at IS NULL AND (name ILIKE $1 OR id::text=$2) ORDER BY name LIMIT 20"),
        "relationship" => sqlx::query_scalar(concat!(
            "SELECT r.id::text FROM work.relationships r WHERE r.archived_at IS NULL AND (",
            super::crm::source_name!(),
            " ILIKE $1 OR r.id::text=$2) ORDER BY r.updated_at DESC LIMIT 20"
        )),
        "task" => sqlx::query_scalar("SELECT id::text FROM work.tasks WHERE archived_at IS NULL AND (title ILIKE $1 OR 'WRK-'||seq=upper($2)) ORDER BY created_at DESC LIMIT 60"),
        _ => return Err(Error::InvalidCode("ENTITY_KIND_UNKNOWN")),
    }
    .bind(&like)
    .bind(&term)
    .fetch_all(&mut *c)
    .await?;
    // documents and cards: only what the caller may open (restricted documents,
    // other people's personal boards)
    let visible: Option<Vec<String>> = match q.kind.as_str() {
        "doc" => Some(
            sqlx::query_scalar("SELECT id FROM unnest($1::text[]) AS x(id) WHERE work.doc_level(id::uuid, $2, $3) > 0")
                .bind(&ids)
                .bind(st.actor.user)
                .bind(st.role.team())
                .fetch_all(&mut *c)
                .await?,
        ),
        "task" => Some(
            sqlx::query_scalar(concat!(
                "SELECT t.id::text FROM work.tasks t WHERE t.id::text = ANY($1) AND ",
                crate::work_card_visible!("$2")
            ))
            .bind(&ids)
            .bind(st.actor.user)
            .fetch_all(&mut *c)
            .await?,
        ),
        _ => None,
    };
    let ids: Vec<String> = match visible {
        Some(v) => ids.into_iter().filter(|i| v.contains(i)).take(20).collect(),
        None => ids,
    };
    let refs: Vec<Ref> = ids
        .iter()
        .map(|id| Ref {
            kind: q.kind.clone(),
            id: id.clone(),
        })
        .collect();
    let labels = resolve(&mut c, &refs).await?;
    let items: Vec<Value> = refs.iter().filter_map(|r| labels.get(r).cloned()).collect();
    Ok(json!({ "items": items }))
}

async fn h_search(
    State(s): State<AppState>,
    Query(q): Query<SearchQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(search(&s, &h, q).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/staff/work/entities/search", get(h_search))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_validated_by_kind() {
        assert!(parse("dsp", "D-5").is_ok());
        assert!(parse("dsp", "D-").is_err());
        assert!(parse("dsp", "spotify").is_err());
        assert!(parse("release", &Uuid::new_v4().to_string()).is_ok());
        assert!(parse("release", "not-a-uuid").is_err());
        assert!(parse("contract", &Uuid::new_v4().to_string()).is_err());
    }
}
