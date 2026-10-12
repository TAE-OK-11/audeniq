//! The single-statement rewrites of the job lifecycle (completion, failure,
//! lease reclaim, outbox receipt) keep every audit row and fence, and the
//! split staff release queue returns the same rows for "awaiting decision"
//! and for a plain status filter.
use audeniq_core::{operations, storage::ObjectStore};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

mod support;
use support::*;

async fn audits(pool: &PgPool, resource: Uuid, action: &str) -> Vec<(Option<Uuid>, String)> {
    sqlx::query_as(
        "SELECT org_id, reason_code FROM operations.audit_events
          WHERE resource_id=$1 AND action=$2 AND actor_service='audeniq-system' ORDER BY occurred_at",
    )
    .bind(resource)
    .bind(action)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn job_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn enqueue(pool: &PgPool, queue: &str, kind: &str, payload: Value, key: &str) -> Uuid {
    let mut tx = pool.begin().await.unwrap();
    let id = operations::enqueue(&mut tx, queue, kind, &payload, key, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

#[sqlx::test]
async fn job_lifecycle_statements_fence_and_audit(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let store: Arc<dyn ObjectStore> = store;
    let u = user(&app).await;

    // outbox.record: receipt, publish mark, job completion and both audits.
    let aggregate = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    let event = operations::event(&mut tx, u.org, aggregate, "test.event", "qe-event")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let job = operations::claim(&pool, "interactive", "w", 60)
        .await
        .unwrap()
        .unwrap();
    operations::execute(&pool, &store, &job).await.unwrap();
    assert_eq!(job_status(&pool, job.id).await, "SUCCEEDED");
    let published: bool =
        sqlx::query_scalar("SELECT published_at IS NOT NULL FROM operations.outbox WHERE id=$1")
            .bind(event)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(published);
    assert_eq!(
        audits(&pool, aggregate, "outbox.recorded").await,
        vec![(Some(u.org), "INTERNAL_RECEIPT_ONLY".to_string())]
    );
    assert_eq!(
        audits(&pool, job.id, "job.succeeded").await,
        vec![(None, "INTERNAL_RECEIPT_ONLY".to_string())]
    );
    // The stale run cannot complete twice.
    assert!(operations::execute(&pool, &store, &job).await.is_err());

    // A receipt for an event that does not exist leaves the job running
    // (the worker then fails it) and writes nothing.
    let missing = Uuid::new_v4();
    let id = enqueue(
        &pool,
        "interactive",
        "outbox.record",
        json!({"event_id": missing}),
        "qe-missing",
    )
    .await;
    let job = operations::claim(&pool, "interactive", "w", 60)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.id, id);
    assert!(operations::execute(&pool, &store, &job).await.is_err());
    assert_eq!(job_status(&pool, id).await, "RUNNING");
    assert!(audits(&pool, id, "job.succeeded").await.is_empty());
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM operations.event_receipts WHERE event_id=$1")
            .bind(missing)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(receipts, 0);

    // fail(): retry, then dead letter, each audited once.
    operations::fail(&pool, &job, false, "QE_RETRY")
        .await
        .unwrap();
    assert_eq!(job_status(&pool, id).await, "QUEUED");
    assert_eq!(
        audits(&pool, id, "job.retry").await,
        vec![(None, "QE_RETRY".to_string())]
    );
    assert!(
        operations::fail(&pool, &job, false, "QE_STALE")
            .await
            .is_err()
    );
    sqlx::query("UPDATE operations.jobs SET run_at=now() WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let job = operations::claim(&pool, "interactive", "w", 60)
        .await
        .unwrap()
        .unwrap();
    operations::fail(&pool, &job, true, "QE_PERMANENT")
        .await
        .unwrap();
    assert_eq!(job_status(&pool, id).await, "DEAD_LETTER");
    assert_eq!(
        audits(&pool, id, "job.dead_letter").await,
        vec![(None, "QE_PERMANENT".to_string())]
    );

    // Lease reclaim: one job with attempts left is requeued, one without is
    // dead-lettered, both audited LEASE_EXPIRED.
    let retry = enqueue(&pool, "finance", "qe.noop", json!({"n": 1}), "qe-retry").await;
    let dead = enqueue(&pool, "finance", "qe.noop", json!({"n": 2}), "qe-dead").await;
    sqlx::query("UPDATE operations.jobs SET max_attempts=1 WHERE id=$1")
        .bind(dead)
        .execute(&pool)
        .await
        .unwrap();
    for _ in 0..2 {
        operations::claim_with(&pool, "finance", "w", 60, false)
            .await
            .unwrap()
            .unwrap();
    }
    sqlx::query(
        "UPDATE operations.jobs SET lease_until=now()-interval '1 second' WHERE id=ANY($1)",
    )
    .bind(vec![retry, dead])
    .execute(&pool)
    .await
    .unwrap();
    let again = operations::claim_with(&pool, "finance", "sweeper", 60, true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((again.id, again.attempts), (retry, 2));
    assert_eq!(job_status(&pool, dead).await, "DEAD_LETTER");
    assert_eq!(
        audits(&pool, retry, "job.retry").await,
        vec![(None, "LEASE_EXPIRED".to_string())]
    );
    assert_eq!(
        audits(&pool, dead, "job.dead_letter").await,
        vec![(None, "LEASE_EXPIRED".to_string())]
    );

    // succeed(): fenced by the lease token, audited with the job kind.
    operations::succeed(&pool, &again).await.unwrap();
    assert_eq!(job_status(&pool, retry).await, "SUCCEEDED");
    assert_eq!(
        audits(&pool, retry, "job.succeeded").await,
        vec![(None, "qe.noop".to_string())]
    );
    assert!(operations::succeed(&pool, &again).await.is_err());
    assert_eq!(audits(&pool, retry, "job.succeeded").await.len(), 1);
}

async fn advance(pool: &PgPool, release: Uuid, steps: &[&str]) {
    for status in steps {
        sqlx::query("UPDATE catalog.releases SET status=$2,row_version=row_version+1 WHERE id=$1")
            .bind(release)
            .bind(status)
            .execute(pool)
            .await
            .unwrap();
    }
}

fn ids(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[sqlx::test]
async fn staff_release_queue_pending_and_status_filters(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let reviewer = user(&app).await;
    sqlx::query(
        "INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'REVIEWER','test-operator')",
    )
    .bind(reviewer.user)
    .execute(&pool)
    .await
    .unwrap();
    let review = create_release(&app, &u).await;
    let correction = create_release(&app, &u).await;
    let submitted = create_release(&app, &u).await;
    let _draft = create_release(&app, &u).await;
    advance(
        &pool,
        review,
        &[
            "SUBMITTED",
            "STAGE1_RUNNING",
            "STAGE1_PASSED",
            "STAGE2_RUNNING",
            "STAGE2_REVIEW",
        ],
    )
    .await;
    advance(
        &pool,
        correction,
        &["SUBMITTED", "STAGE1_RUNNING", "STAGE1_CORRECTION"],
    )
    .await;
    advance(&pool, submitted, &["SUBMITTED"]).await;

    let (s, pending) = send(
        &app,
        "GET",
        "/api/staff/releases",
        Value::Null,
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{pending}");
    assert_eq!(ids(&pending), vec![review.to_string()]);
    let (s, explicit) = send(
        &app,
        "GET",
        "/api/staff/releases?status=PENDING&limit=10",
        Value::Null,
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{explicit}");
    assert_eq!(ids(&explicit), vec![review.to_string()]);
    for (status, expected) in [
        ("STAGE2_REVIEW", review),
        ("STAGE1_CORRECTION", correction),
        ("SUBMITTED", submitted),
    ] {
        let (s, v) = send(
            &app,
            "GET",
            &format!("/api/staff/releases?status={status}"),
            Value::Null,
            Some(&reviewer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(ids(&v), vec![expected.to_string()], "{status}");
    }

    let (s, overview) = send(
        &app,
        "GET",
        "/api/staff/overview",
        Value::Null,
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{overview}");
    assert_eq!(overview["review"], 1);
    assert_eq!(overview["correction"], 1);
}
