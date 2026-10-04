//! Automatic Kanban cards from AUDENIQ events. The events are read where they
//! already live — the exception queue adapters (QC failures, metadata
//! conflicts, missing rights documents, DSP rejections, delivery failures,
//! settlement mismatches, dead automation jobs), new customer inquiries,
//! pending second approvals and overdue release reviews — so the Release / QC
//! / Distribution code is not touched.
//!
//! `work.board_rules` says which board and column receive each trigger; only
//! events detected after the rule was switched on become cards. Every card has
//! a `source_key` (event identity) with a unique index, so running the sweep
//! twice, concurrently or after a restart never creates a second card.
//!
//! The sweep runs at most once a minute per process from staff reads (the
//! work summary) and on demand by ADMIN; nothing depends on a scheduler.
use super::{exceptions, sla, tasks};
use crate::error::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use uuid::Uuid;

static LAST: AtomicI64 = AtomicI64::new(0);

struct Rule {
    board: Uuid,
    column: Uuid,
    status: String,
    since: DateTime<Utc>,
}

/// One card to create (if its key is new).
struct Card {
    key: String,
    source: &'static str,
    title: String,
    description: String,
    work_type: &'static str,
    priority: &'static str,
    link: Option<(String, String)>,
}

fn exception_source(kind: &str) -> &'static str {
    match kind {
        "QC_FAILURE" => "qc",
        "MISSING_RIGHTS_DOCUMENT" => "rights",
        "METADATA_CONFLICT" | "DSP_REJECTION" | "DISTRIBUTION_FAILURE" => "distribution",
        "SETTLEMENT_MISMATCH" => "finance",
        _ => "system",
    }
}

fn exception_type(kind: &str) -> &'static str {
    match kind {
        "QC_FAILURE" | "METADATA_CONFLICT" => "RELEASE",
        "MISSING_RIGHTS_DOCUMENT" => "RIGHTS",
        "DSP_REJECTION" | "DISTRIBUTION_FAILURE" => "URGENT_DELIVERY",
        "SETTLEMENT_MISMATCH" => "SETTLEMENT",
        _ => "GENERAL",
    }
}

fn label(kind: &str) -> &'static str {
    match kind {
        "QC_FAILURE" => "QC 실패",
        "METADATA_CONFLICT" => "메타데이터 충돌",
        "MISSING_RIGHTS_DOCUMENT" => "권리 서류 누락",
        "DSP_REJECTION" => "DSP 반려",
        "DISTRIBUTION_FAILURE" => "배급 실패",
        "SETTLEMENT_MISMATCH" => "정산 불일치",
        "AUTOMATION_FAILURE" => "자동화 실패",
        _ => "확인 필요",
    }
}

/// Insert one card unless its key exists; returns the new task id.
async fn insert(c: &mut PgConnection, r: &Rule, card: &Card) -> Result<Option<Uuid>> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let sla_due = sla::task_due(&mut *c, card.work_type, now).await?;
    let max: Option<f64> = sqlx::query_scalar(
        "SELECT max(position) FROM work.tasks WHERE column_id=$1 AND archived_at IS NULL",
    )
    .bind(r.column)
    .fetch_one(&mut *c)
    .await?;
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO work.tasks(id,title,description,work_type,priority,status,created_by,created_at,updated_at,sla_due_at,
                                board_id,column_id,position,source,source_key,tags,
                                started_at,completed_at)
         VALUES($1,$2,$3,$4,$5,$6,NULL,$7,$7,$8,$9,$10,$11,$12,$13,ARRAY['자동']::text[],
                CASE WHEN $6 IN ('IN_PROGRESS','IN_REVIEW') THEN $7 END, CASE WHEN $6 IN ('DONE','CANCELLED') THEN $7 END)
         ON CONFLICT (source_key) WHERE source_key IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(id)
    .bind(card.title.chars().take(200).collect::<String>())
    .bind(card.description.chars().take(8000).collect::<String>())
    .bind(card.work_type)
    .bind(card.priority)
    .bind(&r.status)
    .bind(now)
    .bind(sla_due)
    .bind(r.board)
    .bind(r.column)
    .bind(max.unwrap_or(0.0) + 1024.0)
    .bind(card.source)
    .bind(&card.key)
    .fetch_optional(&mut *c)
    .await?;
    let Some(id) = inserted else { return Ok(None) };
    if let Some((kind, eid)) = &card.link {
        sqlx::query(
            "INSERT INTO work.links(id,subject_kind,subject_id,entity_kind,entity_id,created_by) VALUES($1,'task',$2,$3,$4,NULL)
             ON CONFLICT (subject_kind, subject_id, entity_kind, entity_id) WHERE archived_at IS NULL DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(kind)
        .bind(eid)
        .execute(&mut *c)
        .await?;
    }
    tasks::event(
        &mut *c,
        id,
        None,
        "CREATED",
        Some(&format!("자동 · {}", card.source)),
    )
    .await?;
    crate::operations::audit(
        &mut *c,
        None,
        None,
        Some(id),
        "work.task_auto_created",
        card.source,
        Uuid::new_v4(),
    )
    .await?;
    Ok(Some(id))
}

/// Create the cards that are due. `force` skips the once-a-minute throttle.
pub async fn sweep(pool: &sqlx::PgPool, force: bool) -> Result<usize> {
    let now = Utc::now().timestamp();
    let last = LAST.load(Ordering::Relaxed);
    if !force
        && (now - last < 60
            || LAST
                .compare_exchange(last, now, Ordering::SeqCst, Ordering::Relaxed)
                .is_err())
    {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let rules: HashMap<String, Rule> = sqlx::query(
        "SELECT r.trigger, r.board_id, r.column_id, bc.status, r.active_since
           FROM work.board_rules r JOIN work.boards b ON b.id=r.board_id AND b.archived_at IS NULL
           JOIN work.board_columns bc ON bc.id=r.column_id AND bc.archived_at IS NULL
          WHERE r.enabled",
    )
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|r| {
        (
            r.get::<String, _>("trigger"),
            Rule {
                board: r.get("board_id"),
                column: r.get("column_id"),
                status: r.get("status"),
                since: r.get("active_since"),
            },
        )
    })
    .collect();
    if rules.is_empty() {
        tx.commit().await?;
        return Ok(0);
    }
    let mut created = 0;

    // Exception queue (one card per failure occurrence; resolved items are skipped)
    let items = exceptions::collect(&mut tx, None, None).await?;
    for e in items.iter().filter(|e| !e.closed()) {
        let Some(rule) = rules.get(&e.kind) else {
            continue;
        };
        if e.detected_at < rule.since || e.triage.as_ref().is_some_and(|t| t.task_id.is_some()) {
            continue;
        }
        let card = Card {
            key: format!(
                "exception:{}:{}@{}",
                e.source,
                e.source_id,
                e.detected_at.timestamp()
            ),
            source: exception_source(&e.kind),
            title: format!("[{}] {}", label(&e.kind), e.title),
            description: [Some(e.detail.clone()), e.org_name.clone(), e.dsp.clone()]
                .into_iter()
                .flatten()
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            work_type: exception_type(&e.kind),
            priority: match e.severity.as_str() {
                "CRITICAL" => "URGENT",
                "HIGH" => "HIGH",
                "LOW" => "LOW",
                _ => "NORMAL",
            },
            link: e.entity_kind.clone().zip(e.entity_id.clone()),
        };
        if let Some(task) = insert(&mut tx, rule, &card).await? {
            created += 1;
            // the exception shows its card (and "to task" is not offered twice)
            sqlx::query(
                "INSERT INTO work.exception_triage(source, source_id, task_id) VALUES($1,$2,$3)
                 ON CONFLICT (source, source_id) DO UPDATE SET task_id=COALESCE(work.exception_triage.task_id, EXCLUDED.task_id), updated_at=now()",
            )
            .bind(&e.source)
            .bind(&e.source_id)
            .bind(task)
            .execute(&mut *tx)
            .await?;
        }
    }

    // New customer inquiries
    if let Some(rule) = rules.get("SUPPORT_INQUIRY") {
        let rows = sqlx::query(
            "SELECT i.id, i.subject, i.category, o.name FROM portal.inquiries i JOIN identity.orgs o ON o.id=i.org_id
              WHERE i.created_at >= $1 AND i.status='OPEN' ORDER BY i.created_at LIMIT 200",
        )
        .bind(rule.since)
        .fetch_all(&mut *tx)
        .await?;
        for r in rows {
            let id: Uuid = r.get("id");
            let card = Card {
                key: format!("inquiry:{id}"),
                source: "support",
                title: format!("[문의] {}", r.get::<String, _>("subject")),
                description: format!(
                    "{} · {}",
                    r.get::<String, _>("name"),
                    r.get::<String, _>("category")
                ),
                work_type: "SUPPORT",
                priority: "NORMAL",
                link: Some(("inquiry".into(), id.to_string())),
            };
            if insert(&mut tx, rule, &card).await?.is_some() {
                created += 1;
            }
        }
    }

    // Second approvals waiting for a reviewer
    if let Some(rule) = rules.get("APPROVAL_REQUIRED") {
        let rows = sqlx::query(
            "SELECT a.id, a.release_id, r.title FROM rights.staff_approvals a JOIN catalog.releases r ON r.id=a.release_id
              WHERE a.status='PENDING' AND a.expires_at>now() AND a.created_at >= $1 ORDER BY a.created_at LIMIT 200",
        )
        .bind(rule.since)
        .fetch_all(&mut *tx)
        .await?;
        for r in rows {
            let id: Uuid = r.get("id");
            let release: Uuid = r.get("release_id");
            let card = Card {
                key: format!("approval:{id}"),
                source: "rights",
                title: format!("[2차 승인] {}", r.get::<String, _>("title")),
                description: "민감 항목 2차 승인이 필요해요.".into(),
                work_type: "RIGHTS",
                priority: "HIGH",
                link: Some(("release".into(), release.to_string())),
            };
            if insert(&mut tx, rule, &card).await?.is_some() {
                created += 1;
            }
        }
    }

    // Release reviews past their SLA
    if let Some(rule) = rules.get("RELEASE_REVIEW_OVERDUE") {
        let book = sla::Book::load(&mut tx).await?;
        for q in sla::queue_items(&mut tx, &book).await? {
            if q.queue != "RELEASE_REVIEW" || q.state != sla::State3::Breached || q.due < rule.since
            {
                continue;
            }
            let card = Card {
                key: format!("review-overdue:{}@{}", q.id, q.start.timestamp()),
                source: "sla",
                title: format!("[심사 지연] {}", q.title),
                description: q.org_name.clone().unwrap_or_default(),
                work_type: "RELEASE",
                priority: "HIGH",
                link: Some(("release".into(), q.id.to_string())),
            };
            if insert(&mut tx, rule, &card).await?.is_some() {
                created += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(created)
}
