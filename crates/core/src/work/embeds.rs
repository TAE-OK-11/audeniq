//! Live data for AUDENIQ blocks embedded in documents (and card / entity
//! previews). A document stores only `(kind, id, view)` and a label; what the
//! block shows — a release's status and UPC, its QC numbers, per-DSP delivery
//! and live state, a ticket, a card — is read here from the owning tables, one
//! batched query per kind and view (never one per block).
//!
//! The caller's access is applied: a card on someone else's personal board, a
//! document the caller cannot open or an id that does not exist all come back
//! as `{"missing": true}`, so an embed cannot be used to probe ids.
use super::entities;
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
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Row};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const MAX_REFS: usize = 60;
pub const VIEWS: &[&str] = &["summary", "qc", "rights", "distribution"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EmbedRef {
    kind: String,
    id: String,
    view: String,
}

impl EmbedRef {
    fn key(&self) -> String {
        if self.view == "summary" {
            format!("{}:{}", self.kind, self.id)
        } else {
            format!("{}:{}:{}", self.kind, self.id, self.view)
        }
    }
}

/// `kind:id[:view],…` — kinds and ids are validated like any entity reference.
fn parse_refs(raw: &str) -> Result<Vec<EmbedRef>> {
    let mut out = BTreeSet::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let mut it = part.splitn(3, ':');
        let (Some(kind), Some(id)) = (it.next(), it.next()) else {
            return Err(Error::InvalidCode("EMBED_REF_INVALID"));
        };
        let view = it.next().unwrap_or("summary");
        if !VIEWS.contains(&view) || (view != "summary" && kind != "release") {
            return Err(Error::InvalidCode("EMBED_VIEW_UNKNOWN"));
        }
        let r = entities::parse(kind, id)?;
        out.insert(EmbedRef {
            kind: r.kind,
            id: r.id,
            view: view.into(),
        });
    }
    if out.len() > MAX_REFS {
        return Err(Error::InvalidCode("TOO_MANY_EMBEDS"));
    }
    Ok(out.into_iter().collect())
}

fn ids_of(refs: &[EmbedRef], kind: &str, view: Option<&str>) -> Vec<Uuid> {
    refs.iter()
        .filter(|r| r.kind == kind && view.is_none_or(|v| r.view == v))
        .filter_map(|r| Uuid::parse_str(&r.id).ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// `integrated_lufs=-9.1 true_peak_dbtp=-0.8 …` → (-9.1, -0.8)
fn loudness(detail: &str) -> (Option<f64>, Option<f64>) {
    let num = |key: &str| -> Option<f64> {
        let rest = detail.split(key).nth(1)?;
        let n: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.' || *c == '+')
            .collect();
        n.parse().ok()
    };
    (num("integrated_lufs="), num("true_peak_dbtp="))
}

/// Rows of `(id, object)` from a query returning `id text, v jsonb`.
async fn objects(
    c: &mut PgConnection,
    sql: &'static str,
    ids: &[Uuid],
    extra: Option<(Uuid, &str)>,
) -> Result<BTreeMap<String, Value>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut q = sqlx::query(sql).bind(ids);
    if let Some((me, role)) = extra {
        q = q.bind(me).bind(role.to_string());
    }
    Ok(q.fetch_all(&mut *c)
        .await?
        .iter()
        .map(|r| (r.get::<String, _>("id"), r.get::<Value, _>("v")))
        .collect())
}

pub async fn resolve(s: &AppState, h: &HeaderMap, raw: &str) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let refs = parse_refs(raw)?;
    let mut tx = s.pool.begin().await?;
    // catalog / delivery rows are read across workspaces (RLS staff read)
    sqlx::query("SELECT set_config('app.staff','on',true)")
        .execute(&mut *tx)
        .await?;
    let me = st.actor.user;
    let role = st.role.as_str();

    let releases = objects(
        &mut tx,
        "SELECT r.id::text AS id, jsonb_build_object('title', r.title, 'status', r.status, 'upc', r.upc, 'release_type', r.release_type,
                'org_name', o.name, 'artist', ar.body #>> '{release,draft,artist}', 'release_date', ar.body #>> '{release,draft,release_date}') AS v
           FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
           LEFT JOIN catalog.application_revisions ar ON ar.org_id=r.org_id AND ar.id=r.current_revision_id
          WHERE r.id = ANY($1)",
        &ids_of(&refs, "release", None),
        None,
    )
    .await?;

    let qc_ids = ids_of(&refs, "release", Some("qc"));
    let qc_checks = objects(
        &mut tx,
        "WITH latest AS (
           SELECT DISTINCT ON (r.id, cr.check_code, cr.result_hash) r.id AS release_id, cr.check_code, cr.status, cr.detail
             FROM catalog.releases r JOIN operations.check_results cr ON cr.revision_id=r.current_revision_id
            WHERE r.id = ANY($1)
            ORDER BY r.id, cr.check_code, cr.result_hash, cr.created_at DESC, cr.id DESC)
         SELECT release_id::text AS id, jsonb_build_object(
                'total', count(*), 'passed', count(*) FILTER (WHERE status IN ('PASS','NOT_APPLICABLE')),
                'blocked', count(*) FILTER (WHERE status='BLOCKED'),
                'issues', COALESCE(jsonb_agg(jsonb_build_object('code', check_code, 'status', status, 'detail', left(detail, 240))
                                   ORDER BY status, check_code) FILTER (WHERE status NOT IN ('PASS','NOT_APPLICABLE')), '[]'),
                'loudness', COALESCE(jsonb_agg(detail) FILTER (WHERE detail LIKE '%integrated_lufs=%'), '[]')) AS v
           FROM latest GROUP BY release_id",
        &qc_ids,
        None,
    )
    .await?;
    let qc_audio = objects(
        &mut tx,
        "SELECT t.release_id::text AS id, jsonb_build_object(
                'tracks', count(*),
                'sample_rates', COALESCE(jsonb_agg(DISTINCT a.sample_rate) FILTER (WHERE a.sample_rate IS NOT NULL), '[]'),
                'channels', COALESCE(jsonb_agg(DISTINCT a.channels) FILTER (WHERE a.channels IS NOT NULL), '[]'),
                'bits_per_sample', COALESCE(jsonb_agg(DISTINCT a.bits_per_sample) FILTER (WHERE a.bits_per_sample IS NOT NULL), '[]'),
                'asset_blocked', count(*) FILTER (WHERE a.qc_status='BLOCKED'),
                'asset_pending', count(*) FILTER (WHERE a.qc_status='PENDING' OR a.id IS NULL)) AS v
           FROM catalog.tracks t LEFT JOIN catalog.assets a ON a.id=t.asset_id
          WHERE t.release_id = ANY($1) GROUP BY t.release_id",
        &qc_ids,
        None,
    )
    .await?;

    let rights = objects(
        &mut tx,
        "SELECT r.id::text AS id, jsonb_build_object(
                'status', r.status, 'rights_epoch', r.rights_epoch,
                'agreement', (SELECT d.status FROM portal.documents d WHERE d.org_id=r.org_id AND d.release_id=r.id AND d.kind='AGREEMENT' LIMIT 1),
                'pending_approvals', (SELECT count(*) FROM rights.staff_approvals a WHERE a.release_id=r.id AND a.status='PENDING' AND a.expires_at>now()),
                'claimed_by', (SELECT c.claimed_by FROM rights.review_claims c WHERE c.release_id=r.id),
                'claimed_at', (SELECT c.claimed_at FROM rights.review_claims c WHERE c.release_id=r.id),
                'open_checks', (SELECT count(*) FROM (SELECT DISTINCT ON (cr.check_code) cr.status FROM operations.check_results cr
                                  WHERE cr.revision_id=r.current_revision_id ORDER BY cr.check_code, cr.created_at DESC) x
                                 WHERE x.status IN ('REVIEW_REQUIRED','BLOCKED','CORRECTION_REQUIRED'))) AS v
           FROM catalog.releases r WHERE r.id = ANY($1)",
        &ids_of(&refs, "release", Some("rights")),
        None,
    )
    .await?;

    let distribution = objects(
        &mut tx,
        "SELECT s.release_id::text AS id, jsonb_agg(jsonb_build_object(
                'dsp', s.dsp_code, 'name', d.name, 'readiness', s.readiness, 'approval', s.approval,
                'delivery', j.status, 'delivered_at', CASE WHEN j.status='DELIVERED' THEN j.updated_at END,
                'updated_at', COALESCE(j.updated_at, s.staged_at), 'error', left(j.last_error, 200),
                'live', lb.live_status) ORDER BY d.name) AS v
           FROM (SELECT DISTINCT ON (release_id, dsp_code) * FROM distribution.delivery_staging
                  WHERE release_id = ANY($1) ORDER BY release_id, dsp_code, staged_at DESC) s
           JOIN distribution.dsp_registry d ON d.code=s.dsp_code
           LEFT JOIN execution.delivery_jobs j ON j.package_id=s.package_id AND j.partner_id=s.dsp_code
           LEFT JOIN execution.live_bindings lb ON lb.package_id=s.package_id AND lb.partner_id=s.dsp_code
          GROUP BY s.release_id",
        &ids_of(&refs, "release", Some("distribution")),
        None,
    )
    .await?;

    let tracks = objects(
        &mut tx,
        "SELECT t.id::text AS id, jsonb_build_object('title', t.title, 'isrc', t.isrc, 'release_id', t.release_id, 'release_title', r.title,
                'artist', ar.name, 'sample_rate', a.sample_rate, 'channels', a.channels, 'duration_secs', a.duration_secs,
                'qc_status', a.qc_status) AS v
           FROM catalog.tracks t JOIN catalog.releases r ON r.id=t.release_id JOIN catalog.artists ar ON ar.id=t.artist_id
           LEFT JOIN catalog.assets a ON a.id=t.asset_id WHERE t.id = ANY($1)",
        &ids_of(&refs, "track", None),
        None,
    )
    .await?;

    let artists = objects(
        &mut tx,
        "SELECT a.id::text AS id, jsonb_build_object('name', a.name, 'org_name', o.name,
                'releases', (SELECT count(DISTINCT t.release_id) FROM catalog.tracks t WHERE t.artist_id=a.id)) AS v
           FROM catalog.artists a JOIN identity.orgs o ON o.id=a.org_id WHERE a.id = ANY($1)",
        &ids_of(&refs, "artist", None),
        None,
    )
    .await?;

    let dsp_codes: Vec<String> = refs
        .iter()
        .filter(|r| r.kind == "dsp")
        .map(|r| r.id.clone())
        .collect();
    let dsps: BTreeMap<String, Value> = if dsp_codes.is_empty() {
        BTreeMap::new()
    } else {
        sqlx::query(
            "SELECT d.code AS id, jsonb_build_object('name', d.name, 'region', d.region, 'status', d.status,
                    'delivered_30d', count(j.id) FILTER (WHERE j.status='DELIVERED'),
                    'failed_30d', count(j.id) FILTER (WHERE j.status IN ('FAILED','DEAD_LETTER')),
                    'in_flight', count(j.id) FILTER (WHERE j.status IN ('QUEUED','LEASED','SENDING','AWAITING_RECONCILIATION')),
                    'last_delivered_at', max(j.updated_at) FILTER (WHERE j.status='DELIVERED'),
                    'last_error', (SELECT left(x.last_error, 200) FROM execution.delivery_jobs x
                                    WHERE x.partner_id=d.code AND x.status IN ('FAILED','DEAD_LETTER') ORDER BY x.updated_at DESC LIMIT 1)) AS v
               FROM distribution.dsp_registry d
               LEFT JOIN execution.delivery_jobs j ON j.partner_id=d.code AND j.created_at > now() - interval '30 days'
              WHERE d.code = ANY($1) GROUP BY d.code, d.name, d.region, d.status",
        )
        .bind(&dsp_codes)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|r| (r.get::<String, _>("id"), r.get::<Value, _>("v")))
        .collect()
    };

    let inquiries = objects(
        &mut tx,
        "SELECT q.id::text AS id, jsonb_build_object('subject', q.subject, 'status', q.status, 'category', q.category,
                'org_name', o.name, 'created_at', q.created_at, 'updated_at', q.updated_at,
                'task', (SELECT jsonb_build_object('id', t.id, 'priority', t.priority, 'assignee_id', t.assignee_id, 'status', t.status)
                           FROM work.links l JOIN work.tasks t ON t.id=l.subject_id
                          WHERE l.subject_kind='task' AND l.entity_kind='inquiry' AND l.entity_id=q.id::text
                            AND l.archived_at IS NULL AND t.archived_at IS NULL
                          ORDER BY (t.status IN ('DONE','CANCELLED')), t.created_at DESC LIMIT 1)) AS v
           FROM portal.inquiries q JOIN identity.orgs o ON o.id=q.org_id WHERE q.id = ANY($1)",
        &ids_of(&refs, "inquiry", None),
        None,
    )
    .await?;

    let tasks = objects(
        &mut tx,
        concat!(
            "SELECT t.id::text AS id, jsonb_build_object('seq', t.seq, 'title', t.title, 'priority', t.priority, 'status', t.status,
                    'column', bc.name, 'board', b.name, 'board_id', t.board_id, 'assignee_id', t.assignee_id, 'due_at', t.due_at,
                    'source', t.source) AS v
               FROM work.tasks t LEFT JOIN work.board_columns bc ON bc.id=t.column_id LEFT JOIN work.boards b ON b.id=t.board_id
              WHERE t.id = ANY($1) AND t.archived_at IS NULL AND ($3::text IS NOT NULL) AND ",
            crate::work_card_visible!("$2")
        ),
        &ids_of(&refs, "task", None),
        Some((me, role)),
    )
    .await?;

    let approvals = objects(
        &mut tx,
        "SELECT a.id::text AS id, jsonb_build_object('status', CASE WHEN a.status='PENDING' AND a.expires_at<=now() THEN 'EXPIRED' ELSE a.status END,
                'release_id', a.release_id, 'release_title', r.title, 'check_codes', a.check_codes, 'reason', left(a.reason, 300),
                'requested_by', a.requested_by, 'decided_by', a.decided_by, 'decided_at', a.decided_at,
                'created_at', a.created_at, 'expires_at', a.expires_at) AS v
           FROM rights.staff_approvals a JOIN catalog.releases r ON r.id=a.release_id WHERE a.id = ANY($1)",
        &ids_of(&refs, "approval", None),
        None,
    )
    .await?;

    let docs = objects(
        &mut tx,
        "SELECT d.id::text AS id, jsonb_build_object('title', d.title, 'icon', d.icon, 'status', d.status,
                'updated_at', d.updated_at, 'updated_by', d.updated_by, 'archived', d.archived_at IS NOT NULL) AS v
           FROM work.docs d WHERE d.id = ANY($1) AND work.doc_level(d.id, $2, $3) > 0",
        &ids_of(&refs, "doc", None),
        Some((me, role)),
    )
    .await?;

    // Other kinds show their label from the entity layer.
    let others: Vec<entities::Ref> = refs
        .iter()
        .filter(|r| {
            !matches!(
                r.kind.as_str(),
                "release" | "track" | "artist" | "dsp" | "inquiry" | "task" | "approval" | "doc"
            )
        })
        .map(|r| entities::Ref {
            kind: r.kind.clone(),
            id: r.id.clone(),
        })
        .collect();
    let labels = entities::resolve(&mut tx, &others).await?;
    tx.commit().await?;

    let mut out = Map::new();
    for r in &refs {
        let found: Option<Value> = match (r.kind.as_str(), r.view.as_str()) {
            ("release", view) => releases.get(&r.id).cloned().map(|mut base| {
                match view {
                    "qc" => {
                        let checks = qc_checks.get(&r.id).cloned().unwrap_or(Value::Null);
                        let (mut lufs, mut peak) = (Vec::new(), Vec::new());
                        for d in checks["loudness"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                            let (l, p) = loudness(d);
                            lufs.extend(l);
                            peak.extend(p);
                        }
                        let min = |v: &[f64]| v.iter().copied().reduce(f64::min);
                        let max = |v: &[f64]| v.iter().copied().reduce(f64::max);
                        base["qc"] = json!({
                            "checks": { "total": checks["total"], "passed": checks["passed"], "blocked": checks["blocked"] },
                            "issues": checks["issues"],
                            "loudness": { "min_lufs": min(&lufs), "max_lufs": max(&lufs), "max_true_peak": max(&peak) },
                            "audio": qc_audio.get(&r.id).cloned().unwrap_or(Value::Null),
                        });
                    }
                    "rights" => base["rights"] = rights.get(&r.id).cloned().unwrap_or(Value::Null),
                    "distribution" => base["distribution"] = distribution.get(&r.id).cloned().unwrap_or_else(|| json!([])),
                    _ => {}
                }
                base
            }),
            ("track", _) => tracks.get(&r.id).cloned(),
            ("artist", _) => artists.get(&r.id).cloned(),
            ("dsp", _) => dsps.get(&r.id).cloned(),
            ("inquiry", _) => inquiries.get(&r.id).cloned(),
            ("task", _) => tasks.get(&r.id).cloned(),
            ("approval", _) => approvals.get(&r.id).cloned(),
            ("doc", _) => docs.get(&r.id).cloned(),
            (kind, _) => labels
                .get(&entities::Ref {
                    kind: kind.into(),
                    id: r.id.clone(),
                })
                .cloned(),
        };
        let mut v = found.unwrap_or_else(|| json!({ "missing": true }));
        v["kind"] = json!(r.kind);
        v["id"] = json!(r.id);
        v["view"] = json!(r.view);
        out.insert(r.key(), v);
    }
    Ok(json!({ "items": out }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedQuery {
    pub refs: String,
}

async fn h_resolve(
    State(s): State<AppState>,
    Query(q): Query<EmbedQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(resolve(&s, &h, &q.refs).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/staff/work/embeds", get(h_resolve))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs_and_views() {
        let id = Uuid::new_v4();
        let refs = parse_refs(&format!(
            "release:{id}:qc, release:{id},dsp:D-1,release:{id}:qc"
        ))
        .unwrap();
        assert_eq!(refs.len(), 3);
        assert!(parse_refs(&format!("task:{id}:qc")).is_err());
        assert!(parse_refs("release:not-a-uuid").is_err());
        assert!(parse_refs("secret:abc").is_err());
        let many: Vec<String> = (0..61)
            .map(|_| format!("task:{}", Uuid::new_v4()))
            .collect();
        assert!(parse_refs(&many.join(",")).is_err());
    }

    #[test]
    fn reads_loudness() {
        assert_eq!(
            loudness("integrated_lufs=-9.1 true_peak_dbtp=-0.8"),
            (Some(-9.1), Some(-0.8))
        );
        assert_eq!(
            loudness("warning (not blocking): integrated_lufs=-6.0 true_peak_dbtp=1.2; streaming"),
            (Some(-6.0), Some(1.2))
        );
        assert_eq!(
            loudness("integrated_lufs=-inf (no gated programme loudness)").0,
            None
        );
    }
}
