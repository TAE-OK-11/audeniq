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
    for queue in ["interactive", "qc", "rights", "distribution", "finance"] {
        let key = format!("QUEUE_{}_CONCURRENCY", queue.to_uppercase());
        let count = std::env::var(key)
            .unwrap_or_else(|_| if queue == "interactive" { "1" } else { "0" }.into())
            .parse::<usize>()?;
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
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=tasks.join_next()=>{anyhow::bail!("worker task unexpectedly exited")}}
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}
