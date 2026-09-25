use audeniq_core::{database, operations, storage};
use std::sync::Arc;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let pool = database::connect(&std::env::var("DATABASE_URL")?, 4).await?;
    let store: Arc<dyn storage::ObjectStore> =
        storage::store_from_env(std::env::var("ALLOW_HTTP_STORAGE").as_deref() == Ok("true"))?;
    let mut tasks = tokio::task::JoinSet::new();
    for queue in [
        "interactive",
        "qc",
        "rights",
        "distribution",
        "finance",
        "delivery",
    ] {
        let key = format!("QUEUE_{}_CONCURRENCY", queue.to_uppercase());
        let count = std::env::var(key)
            .unwrap_or_else(|_| {
                if queue == "interactive" || queue == "delivery" {
                    "1"
                } else {
                    "0"
                }
                .into()
            })
            .parse::<usize>()?;
        anyhow::ensure!(count <= 4, "each queue concurrency <=4 in Foundation");
        anyhow::ensure!(count <= 4, "each queue concurrency <=4 in Foundation");
        for n in 0..count {
            let pool = pool.clone();
            let store = store.clone();
            let name = format!("{}:{queue}:{n}", uuid::Uuid::new_v4());
            tasks.spawn(async move {
                loop {
                    match operations::claim(&pool, queue, &name, 60).await {
                        Ok(Some(job)) => {
                            if operations::execute(&pool, &store, &job).await.is_err() {
                                let _ =
                                    operations::fail(&pool, &job, false, "INTERNAL_HANDLER_ERROR")
                                        .await;
                                tracing::warn!(job_id=%job.id,queue,"job handler failed");
                            }
                        }
                        Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                        Err(_) => {
                            tracing::warn!(queue, "queue polling failed");
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        }
                    }
                }
            });
        }
    }
    // E-5: periodic delivery.reconcile sweep. At most one pending at a time;
    // the 15-minute bucket in the idempotency key keeps each tick's row
    // distinct so a SUCCEEDED sweep never shadows the next tick.
    {
        let pool = pool.clone();
        tasks.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(900)).await;
                let bucket = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() / 900)
                    .unwrap_or(0);
                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(e) => {
                        tracing::warn!(error=%e, "reconcile scheduler: db unavailable");
                        continue;
                    }
                };
                let pending: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM operations.jobs WHERE queue='delivery' AND kind='delivery.reconcile' AND status IN ('QUEUED','RUNNING'))",
                )
                .fetch_one(&mut *tx)
                .await
                .unwrap_or(true);
                if !pending {
                    let key = format!("delivery.reconcile:sched:{bucket}");
                    if let Err(e) = operations::enqueue(
                        &mut tx,
                        "delivery",
                        "delivery.reconcile",
                        &serde_json::json!({}),
                        &key,
                        None,
                    )
                    .await
                    {
                        tracing::warn!(error=%e, "reconcile scheduler: enqueue failed");
                    }
                }
                if let Err(e) = tx.commit().await {
                    tracing::warn!(error=%e, "reconcile scheduler: commit failed");
                }
            }
        });
    }
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=tasks.join_next()=>{anyhow::bail!("worker task unexpectedly exited")}}
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}
