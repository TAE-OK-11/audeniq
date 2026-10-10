//! DSP operations: one screen per DSP built only from what exists — the
//! registry and contract route / onboarding ([`staff::dsps`]), delivery jobs,
//! partner rejections and staging (last 30 days), the open exceptions of the
//! DSP, its CRM relationship (owner, contacts, last contact) and its own SLA
//! policies. No table of its own.
use super::{crm, exceptions, sla::Book};
use crate::{api::AppState, error::Result, staff};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
    routing::get,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;

const WINDOW_DAYS: i32 = 30;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OpsQuery {
    /// D-code: also return that DSP's recent failures and open exceptions
    pub dsp: Option<String>,
}

pub async fn overview(s: &AppState, h: &HeaderMap, q: OpsQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let base = staff::dsps(s, h).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT set_config('app.staff','on',true)")
        .execute(&mut *tx)
        .await?;
    let jobs: HashMap<String, Value> = sqlx::query(
        "SELECT partner_id,
                count(*) AS total,
                count(*) FILTER (WHERE status='DELIVERED') AS delivered,
                count(*) FILTER (WHERE status IN ('FAILED','DEAD_LETTER')) AS failed,
                count(*) FILTER (WHERE status IN ('QUEUED','LEASED','SENDING','AWAITING_RECONCILIATION')) AS in_flight,
                (avg(extract(epoch FROM updated_at - created_at)) FILTER (WHERE status='DELIVERED') / 60)::float8 AS avg_minutes,
                max(updated_at) FILTER (WHERE status='DELIVERED') AS last_delivered_at,
                max(updated_at) FILTER (WHERE status IN ('FAILED','DEAD_LETTER')) AS last_failure_at
           FROM execution.delivery_jobs WHERE created_at > now() - make_interval(days => $1)
          GROUP BY partner_id",
    )
    .bind(WINDOW_DAYS)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|r| {
        let total: i64 = r.get("total");
        let failed: i64 = r.get("failed");
        (
            r.get::<String, _>("partner_id"),
            json!({
                "total": total, "delivered": r.get::<i64,_>("delivered"), "failed": failed,
                "in_flight": r.get::<i64,_>("in_flight"),
                "failure_rate": if total > 0 { failed as f64 / total as f64 } else { 0.0 },
                "avg_minutes": r.get::<Option<f64>,_>("avg_minutes"),
                "last_delivered_at": r.get::<Option<chrono::DateTime<Utc>>,_>("last_delivered_at"),
                "last_failure_at": r.get::<Option<chrono::DateTime<Utc>>,_>("last_failure_at"),
            }),
        )
    })
    .collect();
    let rejections: HashMap<String, i64> = sqlx::query(
        "SELECT j.partner_id, count(*) AS n FROM execution.reconciliation_cases c
           JOIN execution.delivery_jobs j ON j.id=c.job_id
          WHERE c.kind='PARTNER_REJECTED' AND c.created_at > now() - make_interval(days => $1)
          GROUP BY j.partner_id",
    )
    .bind(WINDOW_DAYS)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|r| (r.get("partner_id"), r.get("n")))
    .collect();
    let staging: HashMap<String, (i64, i64)> = sqlx::query(
        "SELECT s.dsp_code,
                count(*) FILTER (WHERE s.approval='PENDING' AND s.readiness<>'CONTENT_BLOCKED') AS pending,
                count(*) FILTER (WHERE s.readiness='CONTENT_BLOCKED') AS blocked
           FROM distribution.delivery_staging s
           JOIN catalog.releases r ON r.id=s.release_id AND r.current_revision_id=s.revision_id
          WHERE r.status<>'WITHDRAWN' GROUP BY s.dsp_code",
    )
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|r| (r.get("dsp_code"), (r.get("pending"), r.get("blocked"))))
    .collect();
    let excs = exceptions::collect(&mut tx, None, None).await?;
    let book = Book::load(&mut tx).await?;
    let codes: Vec<String> = base["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["code"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let rels = crm::by_entities(&mut tx, "dsp", &codes).await?;
    let focus = q.dsp.as_deref().filter(|d| codes.iter().any(|c| c == d));
    let failures: Vec<Value> = match focus {
        Some(d) => sqlx::query_scalar(
            "SELECT jsonb_build_object('job_id',j.id,'status',j.status,'attempts',j.attempts,'last_error',left(j.last_error,300),
                    'updated_at',j.updated_at,'release_id',r.id,'title',r.title,'org_name',o.name)
               FROM execution.delivery_jobs j JOIN identity.orgs o ON o.id=j.org_id
               LEFT JOIN distribution.distribution_packages p ON p.id=j.package_id
               LEFT JOIN distribution.canonical_releases cr ON cr.id=p.canonical_release_id
               LEFT JOIN catalog.releases r ON r.id=cr.release_id
              WHERE j.partner_id=$1 AND j.status IN ('FAILED','DEAD_LETTER') ORDER BY j.updated_at DESC LIMIT 20",
        )
        .bind(d)
        .fetch_all(&mut *tx)
        .await?,
        None => Vec::new(),
    };
    tx.commit().await?;

    let now = Utc::now();
    let open: Vec<&exceptions::Item> = excs.iter().filter(|e| !e.closed()).collect();
    let items: Vec<Value> = base["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut d| {
            let code = d["code"].as_str().unwrap_or("").to_string();
            let mine: Vec<&&exceptions::Item> = open
                .iter()
                .filter(|e| e.dsp.as_deref() == Some(code.as_str()))
                .collect();
            let (pending, blocked) = staging.get(&code).copied().unwrap_or((0, 0));
            d["metrics"] = jobs.get(&code).cloned().unwrap_or_else(
                || json!({"total": 0, "delivered": 0, "failed": 0, "in_flight": 0, "failure_rate": 0.0}),
            );
            d["metrics"]["rejected"] = json!(rejections.get(&code).copied().unwrap_or(0));
            d["staging"] = json!({"pending": pending, "blocked": blocked});
            d["exceptions"] = json!({
                "open": mine.len(),
                "critical": mine.iter().filter(|e| e.severity == "CRITICAL").count(),
                "high": mine.iter().filter(|e| e.severity == "HIGH").count(),
            });
            d["relationship"] = rels.get(&code).cloned().unwrap_or(Value::Null);
            d["sla"] = json!(
                book.policies
                    .iter()
                    .filter(|p| p.scope_id.as_deref() == Some(code.as_str()) && p.enabled)
                    .map(|p| json!({"type": p.work_type, "target_minutes": p.target_minutes, "calendar_id": p.calendar_id}))
                    .collect::<Vec<_>>()
            );
            if focus == Some(code.as_str()) {
                d["recent_failures"] = json!(failures);
                d["open_exceptions"] = json!(
                    mine.iter()
                        .map(|e| e.json(&st, exceptions::sla_of(&book, e, now)))
                        .collect::<Vec<_>>()
                );
            }
            d
        })
        .collect();
    Ok(json!({
        "items": items,
        "window_days": WINDOW_DAYS,
        "live_transmission": base["live_transmission"],
    }))
}

async fn h_overview(
    State(s): State<AppState>,
    Query(q): Query<OpsQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(overview(&s, &h, q).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/staff/work/dsp-ops", get(h_overview))
}
