//! Durable, bounded optional work. Invoked by the worker, never the API.
use crate::{image_encoder, storage::ObjectStore};
use sqlx::{PgPool, Row};
use std::{path::Path, sync::Arc};
use uuid::Uuid;

/// One codec child across all worker replicas. Transaction-scoped for poolers.
const LOCK: i64 = 0x4155_4449_4d47;

/// Caller samples cgroup pressure and obtains worker capacity before calling.
/// Returns false if idle/another replica is encoding. Crash recovery is safe:
/// RUNNING slots are reclaimed only while holding the cluster-wide lock.
pub async fn tick(
    pool: &PgPool,
    storage: &Arc<dyn ObjectStore>,
    antivirus_enabled: bool,
    executable: &Path,
) -> anyhow::Result<bool> {
    let mut guard = pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(LOCK)
        .fetch_one(&mut *guard)
        .await?;
    if !locked {
        return Ok(false);
    }
    // Yield to user-facing work, including uploads waiting for audio analysis.
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations.jobs WHERE status='RUNNING' OR (status='QUEUED' AND run_at<=clock_timestamp()))")
        .fetch_one(&mut *guard).await?;
    if busy {
        return Ok(false);
    }
    sqlx::query("UPDATE catalog.image_jobs SET status=CASE WHEN attempts>=3 THEN 'FAILED' ELSE 'QUEUED' END, outcome='WORKER_RESTART' WHERE status='RUNNING'")
        .execute(pool).await?;
    // A separate short transaction durably counts the attempt before encoding.
    let row = sqlx::query("UPDATE catalog.image_jobs SET status='RUNNING',attempts=attempts+1 WHERE slot=(SELECT slot FROM catalog.image_jobs WHERE status='QUEUED' AND attempts<3 AND run_at<=clock_timestamp() ORDER BY queued_at,slot LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING slot,org_id,asset_id,source_sha256,attempts")
        .fetch_optional(pool).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let slot: i16 = row.get("slot");
    let org: Uuid = row.get("org_id");
    let asset: Uuid = row.get("asset_id");
    let sha: String = row.get("source_sha256");
    let attempts: i16 = row.get("attempts");
    let work = async {
        // Register the cheap fallback first. A failed AVIF attempt must not
        // discard a successfully published WebP; retries reuse its row.
        let webp = image_encoder::produce_variant_with(
            pool,
            storage,
            antivirus_enabled,
            org,
            asset,
            &sha,
            image_encoder::WebpOptions::default(),
            executable,
            true,
            image_encoder::Codec::LossyWebp,
        )
        .await;
        let avif = image_encoder::produce_variant_with(
            pool,
            storage,
            antivirus_enabled,
            org,
            asset,
            &sha,
            image_encoder::WebpOptions::default(),
            executable,
            true,
            image_encoder::Codec::Avif,
        )
        .await;
        let webp = webp?;
        let avif = avif?;
        Ok::<_, anyhow::Error>(avif.or(webp))
    };
    tokio::pin!(work);
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(5));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let result = loop {
        tokio::select! {
            result = &mut work => break result,
            _ = heartbeat.tick() => {
                // Losing this transaction cancels the producer (kill-on-drop child).
                sqlx::query("SELECT 1").execute(&mut *guard).await?;
            }
        }
    };
    let (status, outcome) = match result {
        Ok(Some(_)) => ("DONE", "VARIANT_REGISTERED"),
        Ok(None) => ("SKIPPED", "NOT_USEFUL_OR_CODEC_INELIGIBLE"),
        Err(_) if attempts < 3 => ("QUEUED", "ENCODE_RETRY"),
        Err(_) => ("FAILED", "ENCODE_FAILED"),
    };
    // Only bounded labels: never log codec input, DB errors, keys or credentials.
    tracing::info!(%asset, attempts, status, outcome, "image_job_finished");
    sqlx::query("UPDATE catalog.image_jobs SET status=$2,outcome=$3,run_at=clock_timestamp()+interval '5 minutes' * power(2,attempts-1) WHERE slot=$1 AND status='RUNNING'")
        .bind(slot).bind(status).bind(outcome).execute(pool).await?;
    guard.commit().await?;
    Ok(true)
}
