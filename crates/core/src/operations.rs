use crate::error::{Error, Result};
use crate::storage::ObjectStore;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

/// Process-wide MockDSP shared by every delivery handler the dispatcher runs.
/// MockDsp is Arc<Mutex<..>> internally, so clones share partner-side state:
/// a submission accepted by delivery.send is visible to delivery.poll and
/// delivery.takedown in the same worker process. F5's MockDSP is local-only
/// (Accept behavior); F6 replaces this with credential-backed adapters owned
/// by the API layer, where adapter lifecycle and state live.
fn shared_mockdsp() -> Arc<crate::mockdsp::MockDsp> {
    static INSTANCE: OnceLock<Arc<crate::mockdsp::MockDsp>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            Arc::new(crate::mockdsp::MockDsp::new(
                crate::mockdsp::MockBehavior::Accept,
            ))
        })
        .clone()
}
#[allow(clippy::too_many_arguments)]
pub async fn audit(
    c: &mut PgConnection,
    user: Option<Uuid>,
    org: Option<Uuid>,
    resource: Option<Uuid>,
    action: &str,
    reason: &str,
    request: Uuid,
) -> Result<()> {
    sqlx::query("INSERT INTO operations.audit_events(id,actor_user_id,actor_service,org_id,resource_id,action,reason_code,request_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
 .bind(Uuid::new_v4()).bind(user).bind(if user.is_none(){Some("audeniq-system")}else{None}).bind(org).bind(resource).bind(action).bind(reason).bind(request).execute(c).await?;
    Ok(())
}
pub async fn event(
    c: &mut PgConnection,
    org: Uuid,
    aggregate: Uuid,
    kind: &str,
    key: &str,
) -> Result<Uuid> {
    let id:Uuid=sqlx::query_scalar("INSERT INTO operations.outbox(id,org_id,aggregate_id,event_type,payload,idempotency_key) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(idempotency_key) DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key WHERE operations.outbox.org_id=EXCLUDED.org_id AND operations.outbox.aggregate_id=EXCLUDED.aggregate_id AND operations.outbox.event_type=EXCLUDED.event_type AND operations.outbox.payload=EXCLUDED.payload RETURNING id")
 .bind(Uuid::new_v4()).bind(org).bind(aggregate).bind(kind).bind(json!({"resource_id":aggregate})).bind(key).fetch_optional(&mut *c).await?.ok_or(Error::Conflict)?;
    enqueue(
        c,
        "interactive",
        "outbox.record",
        &json!({"event_id":id}),
        &format!("outbox:{id}"),
        None,
    )
    .await?;
    Ok(id)
}
pub async fn enqueue(
    c: &mut PgConnection,
    queue: &str,
    kind: &str,
    payload: &Value,
    key: &str,
    pin: Option<Uuid>,
) -> Result<Uuid> {
    sqlx::query_scalar("INSERT INTO operations.jobs(id,queue,kind,payload,idempotency_key,pinned_revision_id) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(idempotency_key) DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key WHERE operations.jobs.queue=EXCLUDED.queue AND operations.jobs.kind=EXCLUDED.kind AND operations.jobs.payload=EXCLUDED.payload AND operations.jobs.pinned_revision_id IS NOT DISTINCT FROM EXCLUDED.pinned_revision_id RETURNING id")
 .bind(Uuid::new_v4()).bind(queue).bind(kind).bind(payload).bind(key).bind(pin).fetch_optional(c).await?.ok_or(Error::Conflict)
}
#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub token: Uuid,
    pub kind: String,
    pub payload: Value,
    pub attempts: i32,
}
pub async fn claim(
    pool: &PgPool,
    queue: &str,
    worker: &str,
    lease_seconds: i32,
) -> Result<Option<Job>> {
    if !(1..=3600).contains(&lease_seconds) {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    // Exhausted crash leases are dead-lettered, never left permanently RUNNING.
    // stage2 is excluded: the Stage 2 handoff must survive until F3 implements
    // it, even if a worker crashes mid-park.
    let expired=sqlx::query("UPDATE operations.jobs SET status='DEAD_LETTER',lock_token=NULL,lease_until=NULL,dead_lettered_at=now(),last_error='LEASE_EXHAUSTED' WHERE queue=$1 AND status='RUNNING' AND lease_until<=now() AND kind<>'stage2' AND (attempts>=max_attempts OR kind<>'outbox.record') RETURNING id")
 .bind(queue).fetch_all(&mut *tx).await?;
    for r in expired {
        audit(
            &mut tx,
            None,
            None,
            Some(r.get("id")),
            "job.dead_letter",
            "LEASE_EXHAUSTED",
            Uuid::new_v4(),
        )
        .await?;
    }
    // Only safe internal jobs are registered. Unknown jobs never execute and are explicitly DLQ'd below.
    // A crashed stage2 claim is reclaimable (never dead-lettered): the next
    // claim parks it again until F3 implements the kind.
    let r=sqlx::query("WITH candidate AS (SELECT id FROM operations.jobs WHERE queue=$1 AND attempts<max_attempts AND ((status='QUEUED' AND run_at<=now()) OR (status='RUNNING' AND lease_until<=now() AND kind IN ('outbox.record','stage2'))) ORDER BY priority DESC,run_at,id FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE operations.jobs j SET status='RUNNING',attempts=attempts+1,locked_by=$2,lock_token=$3,lease_until=now()+make_interval(secs=>$4) FROM candidate WHERE j.id=candidate.id RETURNING j.id,j.lock_token,j.kind,j.payload,j.attempts")
 .bind(queue).bind(worker).bind(Uuid::new_v4()).bind(lease_seconds as f64).fetch_optional(&mut *tx).await?;
    let job = r.map(|r| Job {
        id: r.get("id"),
        token: r.get("lock_token"),
        kind: r.get("kind"),
        payload: r.get("payload"),
        attempts: r.get("attempts"),
    });
    if let Some(j) = &job {
        audit(
            &mut tx,
            None,
            None,
            Some(j.id),
            "job.claim",
            "LEASE",
            Uuid::new_v4(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(job)
}
pub async fn heartbeat(pool: &PgPool, j: &Job, seconds: i32) -> Result<()> {
    if !(1..=3600).contains(&seconds) {
        return Err(Error::Invalid);
    }
    let n=sqlx::query("UPDATE operations.jobs SET lease_until=now()+make_interval(secs=>$3) WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp()")
 .bind(j.id).bind(j.token).bind(seconds as f64).execute(pool).await?.rows_affected();
    if n == 1 { Ok(()) } else { Err(Error::Conflict) }
}
/// Park a claimed job back to QUEUED without consuming an attempt, for kinds the
/// dispatcher recognizes but this build does not implement yet (stage2 until F3).
/// The payload and idempotency key are untouched so a future build picks the
/// handoff up; the job is never dead-lettered by the parked path. Parking is
/// not an execution attempt, so the attempt counter is reset: the handoff
/// stays claimable indefinitely.
pub async fn park(pool: &PgPool, j: &Job, code: &str, delay_secs: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query("UPDATE operations.jobs SET status='QUEUED',attempts=0,lock_token=NULL,lease_until=NULL,last_error=$3,run_at=now()+make_interval(secs=>$4) WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp()")
        .bind(j.id).bind(j.token).bind(code).bind(delay_secs as f64).execute(&mut *tx).await?.rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    audit(
        &mut tx,
        None,
        None,
        Some(j.id),
        "job.park",
        code,
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn fail(pool: &PgPool, j: &Job, permanent: bool, code: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let r=sqlx::query("UPDATE operations.jobs SET status=CASE WHEN $3 OR attempts>=max_attempts THEN 'DEAD_LETTER' ELSE 'QUEUED' END,dead_lettered_at=CASE WHEN $3 OR attempts>=max_attempts THEN now() END,last_error=$4,run_at=now()+make_interval(secs=>least(3600,power(2,attempts)*5)::double precision),lock_token=NULL,lease_until=NULL WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp() RETURNING status")
 .bind(j.id).bind(j.token).bind(permanent).bind(code).fetch_optional(&mut *tx).await?.ok_or(Error::Conflict)?;
    audit(
        &mut tx,
        None,
        None,
        Some(j.id),
        if r.get::<String, _>("status") == "DEAD_LETTER" {
            "job.dead_letter"
        } else {
            "job.retry"
        },
        code,
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
/// Mark a claimed job SUCCEEDED. The lock_token/lease guard keeps a crashed
/// worker's replacement from double-completing the same job.
pub async fn succeed(pool: &PgPool, j: &Job) -> Result<()> {
    let mut tx = pool.begin().await?;
    let n=sqlx::query("UPDATE operations.jobs SET status='SUCCEEDED',lock_token=NULL,lease_until=NULL WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp()")
 .bind(j.id).bind(j.token).execute(&mut *tx).await?.rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    audit(
        &mut tx,
        None,
        None,
        Some(j.id),
        "job.succeeded",
        &j.kind,
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn execute(pool: &PgPool, storage: &Arc<dyn ObjectStore>, j: &Job) -> Result<()> {
    if j.kind == "stage1" {
        let revision_id = j
            .payload
            .get("revision_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Internal)?;
        match crate::submission::run_stage1(pool, storage, revision_id).await {
            Ok(summary) => {
                if summary.needs_retry {
                    return fail(pool, j, false, "QC_TECHNICAL_RETRY").await;
                }
                return succeed(pool, j).await;
            }
            Err(e) => {
                // last_error is ops-visible: include the underlying cause.
                let short: String = format!("{e:?}").chars().take(500).collect();
                return fail(pool, j, false, &format!("STAGE1_ERROR:{short}")).await;
            }
        }
    }
    if j.kind == "stage2" {
        // F3: the durable stage2.review job. None = lease lost; leave the job
        // alone so the sweeper reclaims it instead of burning an attempt.
        match crate::review::run_stage2(pool, j).await {
            Ok(None) => return Ok(()),
            Ok(Some(summary)) => {
                if summary.needs_retry {
                    return fail(pool, j, false, "STAGE2_TECHNICAL_RETRY").await;
                }
                return succeed(pool, j).await;
            }
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                return fail(pool, j, false, &format!("STAGE2_ERROR:{short}")).await;
            }
        }
    }
    if j.kind == "prepare_release" {
        // F4: the durable prepare_release job (Stage 3 prep: canonical
        // snapshot + frozen package). None = lease lost; leave the job alone
        // so the sweeper reclaims it instead of burning an attempt.
        match crate::distribution::run_prepare_release(pool, storage, j).await {
            Ok(None) => return Ok(()),
            Ok(Some(_)) => return succeed(pool, j).await,
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                // An identifier conflict is provably permanent: the UPC/ISRC
                // is already assigned to a different release or track, so no
                // retry can succeed. Dead-letter for human review instead of
                // burning attempts.
                let permanent = matches!(e, Error::PolicyGate("IDENTIFIER_CONFLICT"));
                return fail(
                    pool,
                    j,
                    permanent,
                    &format!("PREPARE_RELEASE_ERROR:{short}"),
                )
                .await;
            }
        }
    }
    if j.kind == "delivery.enqueue" {
        // F5: fan out one delivery.send per eligible partner for a frozen package.
        let package_id = j
            .payload
            .get("package_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Internal)?;
        match crate::execution::enqueue_delivery_jobs(pool, package_id).await {
            Ok((job_ids, org)) => {
                let mut tx = pool.begin().await?;
                for job_id in job_ids {
                    enqueue(
                        &mut tx,
                        "delivery",
                        "delivery.send",
                        &json!({"delivery_job_id": job_id, "org_id": org}),
                        &format!("delivery.send:{job_id}"),
                        None,
                    )
                    .await?;
                }
                tx.commit().await?;
                return succeed(pool, j).await;
            }
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                return fail(pool, j, false, &format!("DELIVERY_ENQUEUE_ERROR:{short}")).await;
            }
        }
    }
    if j.kind == "delivery.send" {
        // F5: E-0..E-3 for one delivery job. The adapter registry ships the
        // local MockDSP only; real partners register in F6/F9.
        let job_id = j
            .payload
            .get("delivery_job_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Internal)?;
        let org = j
            .payload
            .get("org_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Internal)?;
        // RLS-safe: execution.delivery_jobs is org-scoped, so the status read
        // authorizes the org from the payload before touching the table.
        let terminal = crate::execution::delivery_job_status(pool, org, job_id).await?;
        match terminal.as_deref() {
            Some("DELIVERED")
            | Some("FAILED")
            | Some("DEAD_LETTER")
            | Some("AWAITING_RECONCILIATION") => return succeed(pool, j).await,
            None => return fail(pool, j, true, "DELIVERY_JOB_MISSING").await,
            _ => {}
        }
        let djob =
            match crate::execution::lease_delivery_job(pool, job_id, org, "delivery-worker", 300)
                .await?
            {
                Some(d) => d,
                // Lease lost; leave the job alone so the sweeper reclaims it.
                None => return Ok(()),
            };
        let mut registry = crate::execution::AdapterRegistry::new();
        registry.register(shared_mockdsp());
        let adapter = registry
            .get(&djob.partner_id)
            .ok_or(Error::PolicyGate("EXECUTION_NO_ADAPTER"))?;
        return match crate::execution::run_delivery(pool, storage, adapter.as_ref(), &djob).await {
            Ok(status) => match status.as_str() {
                "DELIVERED" | "AWAITING_RECONCILIATION" => succeed(pool, j).await,
                _ => fail(pool, j, true, &format!("DELIVERY_{status}")).await,
            },
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                let permanent = matches!(e, Error::PolicyGate(_));
                fail(pool, j, permanent, &format!("DELIVERY_ERROR:{short}")).await
            }
        };
    }
    if j.kind == "delivery.poll" {
        // F5: E-4 live-state poll for one (package, partner).
        let package_id = j
            .payload
            .get("package_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Internal)?;
        let partner_id = j
            .payload
            .get("partner_id")
            .and_then(Value::as_str)
            .ok_or(Error::Internal)?;
        let org: Uuid =
            sqlx::query_scalar("SELECT org_id FROM distribution.distribution_packages WHERE id=$1")
                .bind(package_id)
                .fetch_optional(pool)
                .await?
                .ok_or(Error::NotFound)?;
        let mut registry = crate::execution::AdapterRegistry::new();
        registry.register(shared_mockdsp());
        let adapter = registry
            .get(partner_id)
            .ok_or(Error::PolicyGate("EXECUTION_NO_ADAPTER"))?;
        return match crate::execution::poll_live(
            pool,
            org,
            adapter.as_ref(),
            package_id,
            partner_id,
        )
        .await
        {
            Ok(_) => succeed(pool, j).await,
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                fail(pool, j, false, &format!("DELIVERY_POLL_ERROR:{short}")).await
            }
        };
    }
    if j.kind == "delivery.reconcile" {
        // F5: E-5 reconciliation sweep.
        return match crate::execution::reconcile(pool, 3600).await {
            Ok(_) => succeed(pool, j).await,
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                fail(pool, j, false, &format!("DELIVERY_RECONCILE_ERROR:{short}")).await
            }
        };
    }
    if j.kind == "delivery.takedown" {
        let package_id = j
            .payload
            .get("package_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Internal)?;
        let partner_id = j
            .payload
            .get("partner_id")
            .and_then(Value::as_str)
            .ok_or(Error::Internal)?;
        let org: Uuid =
            sqlx::query_scalar("SELECT org_id FROM distribution.distribution_packages WHERE id=$1")
                .bind(package_id)
                .fetch_optional(pool)
                .await?
                .ok_or(Error::NotFound)?;
        let mut registry = crate::execution::AdapterRegistry::new();
        registry.register(shared_mockdsp());
        let adapter = registry
            .get(partner_id)
            .ok_or(Error::PolicyGate("EXECUTION_NO_ADAPTER"))?;
        return match crate::execution::takedown_release(
            pool,
            org,
            adapter.as_ref(),
            package_id,
            partner_id,
        )
        .await
        {
            Ok(_) => succeed(pool, j).await,
            Err(e) => {
                let short: String = format!("{e:?}").chars().take(500).collect();
                let permanent = matches!(e, Error::PolicyGate(_));
                fail(
                    pool,
                    j,
                    permanent,
                    &format!("DELIVERY_TAKEDOWN_ERROR:{short}"),
                )
                .await
            }
        };
    }
    if j.kind != "outbox.record" {
        return fail(pool, j, true, "UNIMPLEMENTED_JOB_KIND").await;
    }
    let event = j
        .payload
        .get("event_id")
        .and_then(Value::as_str)
        .and_then(|v| Uuid::parse_str(v).ok());
    let Some(event) = event else {
        return fail(pool, j, true, "INVALID_JOB_PAYLOAD").await;
    };
    let mut tx = pool.begin().await?;
    let current:Option<Uuid>=sqlx::query_scalar("SELECT id FROM operations.jobs WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp() FOR UPDATE")
 .bind(j.id).bind(j.token).fetch_optional(&mut *tx).await?;
    current.ok_or(Error::Conflict)?;
    let row =
        sqlx::query("SELECT org_id,aggregate_id FROM operations.outbox WHERE id=$1 FOR UPDATE")
            .bind(event)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
    let n=sqlx::query("INSERT INTO operations.event_receipts(event_id,consumer) VALUES($1,'foundation.internal') ON CONFLICT DO NOTHING").bind(event).execute(&mut *tx).await?.rows_affected();
    if n == 1 {
        audit(
            &mut tx,
            None,
            Some(row.get("org_id")),
            Some(row.get("aggregate_id")),
            "outbox.recorded",
            "INTERNAL_RECEIPT_ONLY",
            event,
        )
        .await?;
        sqlx::query("UPDATE operations.outbox SET published_at=now() WHERE id=$1")
            .bind(event)
            .execute(&mut *tx)
            .await?;
    }
    let n=sqlx::query("UPDATE operations.jobs SET status='SUCCEEDED',lock_token=NULL,lease_until=NULL WHERE id=$1 AND lock_token=$2 AND lease_until>clock_timestamp()")
 .bind(j.id).bind(j.token).execute(&mut *tx).await?.rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    audit(
        &mut tx,
        None,
        None,
        Some(j.id),
        "job.succeeded",
        "INTERNAL_RECEIPT_ONLY",
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
/// Append a pinned check result without advancing the pipeline.
/// Owner-only in Foundation. F2 requires explicit minimal grants in deploy/grants.sql
/// and runtime-role integration tests before adding an authorized orchestrator.
pub async fn record_check(
    pool: &PgPool,
    org: Uuid,
    release: Uuid,
    pin: Uuid,
    code: &str,
    rule: &str,
    hash: &str,
) -> Result<String> {
    let mut tx = pool.begin().await?;
    let current: Option<Uuid> = sqlx::query_scalar(
        "SELECT current_revision_id FROM catalog.releases WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let belongs:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.application_revisions WHERE org_id=$1 AND release_id=$2 AND id=$3)").bind(org).bind(release).bind(pin).fetch_one(&mut *tx).await?;
    if !belongs {
        return Err(Error::Forbidden);
    }
    // Foundation records pending technical work only. It cannot manufacture PASS.
    let status = if current == Some(pin) {
        "UNKNOWN"
    } else {
        "STALE"
    };
    sqlx::query("INSERT INTO operations.check_results(id,revision_id,check_code,rule_version,status,result_hash) VALUES($1,$2,$3,$4,$5,$6)")
 .bind(Uuid::new_v4()).bind(pin).bind(code).bind(rule).bind(status).bind(hash).execute(&mut *tx).await?;
    audit(
        &mut tx,
        None,
        Some(org),
        Some(release),
        "check.recorded",
        status,
        Uuid::new_v4(),
    )
    .await?;
    event(
        &mut tx,
        org,
        release,
        "check.recorded",
        &format!("check:{pin}:{code}:{rule}:{hash}"),
    )
    .await?;
    tx.commit().await?;
    Ok(status.into())
}
