//! P2: per-kind job policies (migration 0065) — attempts copied at insert,
//! timeout returned with the claim, policy backoff, and the staff
//! dead-letter requeue.
use audeniq_core::{database, error::Error, operations};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn enqueue(pool: &PgPool, queue: &str, kind: &str) -> Uuid {
    let mut c = pool.acquire().await.unwrap();
    operations::enqueue(
        &mut c,
        queue,
        kind,
        &json!({"n": Uuid::new_v4()}),
        &format!("test:{}", Uuid::new_v4()),
        None,
    )
    .await
    .unwrap()
}

#[sqlx::test]
async fn policy_sets_attempts_timeout_and_backoff(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    // Tuning a row applies to jobs enqueued afterwards.
    sqlx::query("UPDATE operations.job_policies SET max_attempts=2 WHERE kind='delivery.poll'")
        .execute(&pool)
        .await
        .unwrap();
    let id = enqueue(&pool, "delivery", "delivery.poll").await;
    let max: i32 = sqlx::query_scalar("SELECT max_attempts FROM operations.jobs WHERE id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(max, 2);
    // A kind without a policy row keeps the old default.
    let other = enqueue(&pool, "finance", "finance.unlisted").await;
    let max: i32 = sqlx::query_scalar("SELECT max_attempts FROM operations.jobs WHERE id=$1")
        .bind(other)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(max, 5);

    // The claim carries the kind's timeout (network kinds only).
    let job = operations::claim(&pool, "delivery", "t", 60)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.id, id);
    assert_eq!(job.timeout_secs, Some(120));

    // Retry delay = base(30s)·2^attempts(1) = 60 s, ±20% jitter.
    operations::fail(&pool, &job, false, "TEST").await.unwrap();
    let (status, delay): (String, f64) = sqlx::query_as(
        "SELECT status, EXTRACT(EPOCH FROM run_at - now())::float8 FROM operations.jobs WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "QUEUED");
    assert!((47.0..=73.0).contains(&delay), "delay {delay}");

    let stage = enqueue(&pool, "qc", "stage1").await;
    let job = operations::claim(&pool, "qc", "t", 60)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.id, stage);
    assert_eq!(job.timeout_secs, None, "CPU kinds bound themselves");
}

#[sqlx::test]
async fn staff_requeue_of_dead_letters(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let id = enqueue(&pool, "delivery", "delivery.poll").await;
    let job = operations::claim(&pool, "delivery", "t", 60)
        .await
        .unwrap()
        .unwrap();
    operations::fail(&pool, &job, true, "PARTNER_CREDENTIALS")
        .await
        .unwrap();
    let staff = Uuid::new_v4();
    let mut c = pool.acquire().await.unwrap();
    let kind = operations::requeue_dead_letter(&mut c, id, staff, Uuid::new_v4())
        .await
        .unwrap();
    assert_eq!(kind, "delivery.poll");
    let (status, attempts, by): (String, i32, Option<Uuid>) =
        sqlx::query_as("SELECT status, attempts, retried_by FROM operations.jobs WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempts, by), ("QUEUED", 0, Some(staff)));
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE action='staff.job_requeued' AND resource_id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
    // Not dead-lettered (any more): nothing to requeue.
    assert!(matches!(
        operations::requeue_dead_letter(&mut c, id, staff, Uuid::new_v4()).await,
        Err(Error::NotFound)
    ));

    // Pipeline kinds recover through resubmission / review, not a requeue.
    let stage = enqueue(&pool, "qc", "stage1").await;
    sqlx::query(
        "UPDATE operations.jobs SET status='DEAD_LETTER', dead_lettered_at=now() WHERE id=$1",
    )
    .bind(stage)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        operations::requeue_dead_letter(&mut c, stage, staff, Uuid::new_v4()).await,
        Err(Error::PolicyGate("JOB_RETRY_NOT_APPLICABLE"))
    ));
}

#[test]
fn only_invalid_input_is_permanent() {
    assert!(operations::is_permanent(&Error::Invalid));
    assert!(operations::is_permanent(&Error::InvalidCode("X")));
    assert!(!operations::is_permanent(&Error::Storage));
    assert!(!operations::is_permanent(&Error::Internal));
    assert!(!operations::is_permanent(&Error::PolicyGate("CONFIG")));
}
