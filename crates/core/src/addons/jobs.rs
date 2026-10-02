//! Provider preparation adapters. Manual is the production-safe default:
//! no remote delivery, payment, render or database registration is invented.
use super::{
    model::{Order, Status},
    workflow,
};
use crate::{
    auth::Actor,
    error::{Error, Result},
    operations::{self, Job},
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

pub struct WorkItem {
    pub order_id: Uuid,
    pub service_code: String,
    pub kind: String,
    /// Stable across retries. A real provider must accept this dedupe key.
    pub idempotency_key: String,
}
pub enum AdapterOutcome {
    Manual {
        provider: &'static str,
    },
    /// Future providers return a receipt, never assert COMPLETED at dispatch.
    Submitted {
        provider: &'static str,
        reference: String,
    },
    Retryable {
        code: &'static str,
    },
    Permanent {
        code: &'static str,
    },
}
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn prepare(&self, item: &WorkItem) -> Result<AdapterOutcome>;
}
pub struct ManualAdapter;
#[async_trait]
impl ProviderAdapter for ManualAdapter {
    async fn prepare(&self, item: &WorkItem) -> Result<AdapterOutcome> {
        let provider = match item.kind.as_str() {
            "addon.lyrics.sync" => "musixmatch-manual",
            "addon.music_data.submit" => "listenbrainz-manual",
            "addon.profile.process" => "dsp-profile-manual",
            "addon.mv.review_prepare" => "review-agency-manual",
            "addon.mv.distribute" => "video-dsp-manual",
            _ => "audeniq-manual",
        };
        Ok(AdapterOutcome::Manual { provider })
    }
}

async fn preflight(c: &mut PgConnection, o: &Order) -> Result<()> {
    let owner:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.addon_orders x JOIN identity.users u ON u.id=x.requester_user_id JOIN identity.memberships m ON m.org_id=x.org_id AND m.user_id=u.id
        JOIN identity.resource_acl acl ON acl.org_id=x.org_id AND acl.resource_id=CASE WHEN x.target_type='track' THEN x.release_id ELSE x.target_id END AND acl.principal_party_id=u.party_id
        WHERE x.id=$1 AND u.status='ACTIVE' AND m.status='ACTIVE' AND m.role<>'VIEWER' AND acl.action='write' AND acl.revoked_at IS NULL AND acl.starts_at<=now() AND (acl.ends_at IS NULL OR acl.ends_at>now()))").bind(o.id).fetch_one(&mut *c).await?;
    if !owner {
        return Err(Error::PolicyGate("ADDON_REQUESTER_ACCESS_REVOKED"));
    }
    let target:bool=match o.target_type.as_str(){
        "artist"=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.artists WHERE org_id=$1 AND id=$2 AND archived_at IS NULL)").bind(o.org_id).bind(o.target_id).fetch_one(&mut *c).await?,
        "release"=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL AND status NOT IN ('WITHDRAWN','SUPERSEDED'))").bind(o.org_id).bind(o.target_id).fetch_one(&mut *c).await?,
        "track"=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.tracks t JOIN catalog.releases r ON r.id=t.release_id AND r.org_id=t.org_id WHERE t.org_id=$1 AND t.id=$2 AND t.archived_at IS NULL AND r.archived_at IS NULL AND r.status NOT IN ('WITHDRAWN','SUPERSEDED'))").bind(o.org_id).bind(o.target_id).fetch_one(&mut *c).await?,
        "music_video"=>{workflow::asset_ready(c,o.org_id,o.target_id,"VIDEO").await?;true},
        _=>false,
    };
    if !target {
        return Err(Error::PolicyGate("ADDON_TARGET_UNAVAILABLE"));
    }
    if o.service_code == "LYRIC_VIDEO_PLUS" {
        let assets=sqlx::query("SELECT t.asset_id,r.artwork_asset_id FROM catalog.tracks t JOIN catalog.releases r ON r.id=t.release_id AND r.org_id=t.org_id WHERE t.org_id=$1 AND t.id=$2").bind(o.org_id).bind(o.target_id).fetch_one(&mut *c).await?;
        for (field, kind) in [("asset_id", "AUDIO"), ("artwork_asset_id", "IMAGE")] {
            workflow::asset_ready(
                c,
                o.org_id,
                assets
                    .get::<Option<Uuid>, _>(field)
                    .ok_or(Error::PolicyGate("ADDON_ASSET_NOT_VERIFIED"))?,
                kind,
            )
            .await?;
        }
    }
    Ok(())
}

pub fn dispatch<'a>(c: &'a mut PgConnection, o: &'a Order) -> workflow::BoxFuture<'a, ()> {
    Box::pin(dispatch_inner(c, o))
}
async fn dispatch_inner(c: &mut PgConnection, o: &Order) -> Result<()> {
    let kinds: Vec<&str> = match o.service_code.as_str() {
        "PROFILE_BASIC" | "PROFILE_PLUS" => vec!["addon.profile.process"],
        "MIGRATION" => vec!["addon.migration.prepare"],
        "PRIORITY_DELIVERY" => vec!["addon.priority.apply"],
        "LYRICS_BASIC" => {
            let basic: bool = sqlx::query_scalar(
                "SELECT basic_video_requested FROM catalog.lyrics_requests WHERE addon_order_id=$1",
            )
            .bind(o.id)
            .fetch_one(&mut *c)
            .await?;
            if basic {
                vec!["addon.lyrics.sync", "addon.lyric_video.render"]
            } else {
                vec!["addon.lyrics.sync"]
            }
        }
        "AI_SYNC_LYRICS" => vec!["addon.lyrics.sync"],
        "LYRIC_VIDEO_PLUS" => vec!["addon.lyrics.sync", "addon.lyric_video.render"],
        "PROMO_BASIC" => vec!["addon.promo.smartlink", "addon.promo.card"],
        "MV_REVIEW_AND_GLOBAL" => {
            let approved: bool = sqlx::query_scalar(
                "SELECT review_status='APPROVED' FROM catalog.mv_requests WHERE addon_order_id=$1",
            )
            .bind(o.id)
            .fetch_one(&mut *c)
            .await?;
            if approved {
                workflow::evidence_gate(c, o).await?;
                vec!["addon.mv.distribute"]
            } else {
                vec!["addon.mv.review_prepare"]
            }
        }
        "MV_GLOBAL_ONLY" => {
            workflow::evidence_gate(c, o).await?;
            vec!["addon.mv.distribute"]
        }
        "MUSIC_DATA_BASIC" => vec!["addon.music_data.submit"],
        _ => return Err(Error::Invalid),
    };
    let event = Uuid::new_v4();
    let key = format!("addon.dispatch:{}:{}", o.id, o.dispatch_generation);
    let payload = json!({"resource_id":o.id,"generation":o.dispatch_generation,"kinds":kinds});
    let event:Uuid=sqlx::query_scalar("INSERT INTO operations.outbox(id,org_id,aggregate_id,event_type,payload,idempotency_key) VALUES($1,$2,$3,'addon.dispatch',$4,$5) ON CONFLICT(idempotency_key) DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key WHERE operations.outbox.payload=EXCLUDED.payload RETURNING id")
        .bind(event).bind(o.org_id).bind(o.id).bind(payload).bind(key).fetch_optional(&mut *c).await?.ok_or(Error::Conflict)?;
    operations::enqueue(
        c,
        "interactive",
        "outbox.record",
        &json!({"event_id":event}),
        &format!("outbox:{event}"),
        None,
    )
    .await?;
    Ok(())
}
/// Called inside the existing outbox consumer's receipt transaction.
pub async fn consume_event(
    c: &mut PgConnection,
    org: Uuid,
    event: Uuid,
    payload: &Value,
) -> Result<()> {
    let id = payload
        .get("resource_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(Error::Invalid)?;
    let generation = payload["generation"].as_i64().ok_or(Error::Invalid)?;
    let o = workflow::load(c, org, id).await?;
    if o.terminal() || o.status == "FAILED" || i64::from(o.dispatch_generation) != generation {
        return Ok(());
    }
    for kind in payload["kinds"].as_array().ok_or(Error::Invalid)? {
        let kind = kind.as_str().ok_or(Error::Invalid)?;
        let queue = if kind == "addon.mv.distribute" {
            "delivery"
        } else {
            "interactive"
        };
        operations::enqueue(
            c,
            queue,
            kind,
            &json!({"org_id":org,"order_id":id,"generation":generation,"release_id":o.release_id}),
            &format!("addon.job:{event}:{kind}"),
            None,
        )
        .await?;
    }
    Ok(())
}

pub async fn apply_priority(c: &mut PgConnection, a: Option<&Actor>, o: &Order) -> Result<()> {
    let release = o.release_id.ok_or(Error::Invalid)?;
    // No running job is touched; pipeline correctness/leases stay intact.
    sqlx::query("INSERT INTO catalog.addon_release_priorities(org_id,release_id,addon_order_id,priority) VALUES($1,$2,$3,10) ON CONFLICT(release_id) DO UPDATE SET addon_order_id=$3,priority=10 WHERE addon_release_priorities.org_id=$1")
        .bind(o.org_id).bind(release).bind(o.id).execute(&mut *c).await?;
    sqlx::query("UPDATE catalog.addon_orders SET priority=10,row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND priority<>10").bind(o.org_id).bind(o.id).execute(&mut *c).await?;
    let changed=sqlx::query("UPDATE operations.jobs SET addon_priority_previous=coalesce(addon_priority_previous,priority),priority=greatest(priority,10) WHERE release_id=$1 AND queue IN ('qc','rights','distribution','delivery') AND status='QUEUED' AND priority<10 RETURNING id,addon_priority_previous,priority").bind(release).fetch_all(&mut *c).await?;
    for j in changed {
        workflow::audit(
            c,
            a,
            o.org_id,
            j.get("id"),
            "addon.queue.priority",
            "PRIORITY_DELIVERY",
            json!({"priority":j.get::<i32,_>("addon_priority_previous"),"release_id":release}),
            json!({"priority":j.get::<i32,_>("priority"),"order_id":o.id}),
        )
        .await?;
    }
    workflow::audit(
        c,
        a,
        o.org_id,
        o.id,
        "addon.priority.applied",
        "INTERNAL_PRIORITY",
        json!({"priority":"NORMAL"}),
        json!({"priority":"HIGH","release_id":release}),
    )
    .await?;
    Ok(())
}
pub async fn remove_priority(
    c: &mut PgConnection,
    a: Option<&Actor>,
    o: &Order,
    reason: &str,
) -> Result<()> {
    if o.service_code != "PRIORITY_DELIVERY" {
        return Ok(());
    }
    let r = o.release_id.ok_or(Error::Invalid)?;
    let n=sqlx::query("DELETE FROM catalog.addon_release_priorities WHERE org_id=$1 AND release_id=$2 AND addon_order_id=$3").bind(o.org_id).bind(r).bind(o.id).execute(&mut *c).await?.rows_affected();
    if n == 1 {
        let rows=sqlx::query("UPDATE operations.jobs SET priority=addon_priority_previous,addon_priority_previous=NULL WHERE release_id=$1 AND status='QUEUED' AND addon_priority_previous IS NOT NULL AND priority=10 RETURNING id,priority").bind(r).fetch_all(&mut *c).await?;
        for row in rows {
            workflow::audit(
                c,
                a,
                o.org_id,
                row.get("id"),
                "addon.queue.priority",
                reason,
                json!({"priority":10}),
                json!({"priority":row.get::<i32,_>("priority")}),
            )
            .await?;
        }
        workflow::audit(
            c,
            a,
            o.org_id,
            o.id,
            "addon.priority.removed",
            reason,
            json!({"priority":"HIGH"}),
            json!({"priority":"NORMAL"}),
        )
        .await?;
    }
    Ok(())
}
pub fn execute<'a>(pool: &'a PgPool, j: &'a Job) -> workflow::BoxFuture<'a, ()> {
    execute_with(pool, j, &ManualAdapter)
}

pub fn execute_with<'a>(
    pool: &'a PgPool,
    j: &'a Job,
    adapter: &'a dyn ProviderAdapter,
) -> workflow::BoxFuture<'a, ()> {
    Box::pin(execute_inner(pool, j, adapter))
}
async fn execute_inner(pool: &PgPool, j: &Job, adapter: &dyn ProviderAdapter) -> Result<()> {
    let uuid = |k: &str| {
        j.payload[k]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Invalid)
    };
    let org = uuid("org_id")?;
    let id = uuid("order_id")?;
    let generation = j.payload["generation"].as_i64().ok_or(Error::Invalid)?;
    let mut tx = pool.begin().await?;
    // The same job lease fence used by every existing worker handler.
    let held:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations.jobs WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp())").bind(j.id).bind(j.token).fetch_one(&mut *tx).await?;
    if !held {
        return Err(Error::Conflict);
    }
    let o = workflow::load(&mut tx, org, id).await?;
    let runnable = matches!(o.status.as_str(), "IN_PROGRESS" | "EXTERNAL_PENDING")
        || (j.kind == "addon.priority.apply" && o.status == "QUEUED");
    if !runnable || o.terminal() || i64::from(o.dispatch_generation) != generation {
        finish(&mut tx, j).await?;
        tx.commit().await?;
        return Ok(());
    }
    if !matches!(o.payment_status.as_str(), "NOT_REQUIRED" | "PAID") {
        return Err(Error::Conflict);
    }
    if let Err(e) = preflight(&mut tx, &o).await {
        if let Error::PolicyGate(reason) = e {
            workflow::transition(&mut tx, None, org, id, Status::NEEDS_INFO, reason).await?;
            finish(&mut tx, j).await?;
            tx.commit().await?;
            return Ok(());
        }
        return Err(e);
    }
    let prior: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.addon_provider_tasks WHERE job_id=$1)",
    )
    .bind(j.id)
    .fetch_one(&mut *tx)
    .await?;
    if !prior {
        if j.kind == "addon.priority.apply" {
            apply_priority(&mut tx, None, &o).await?;
            if o.status == "QUEUED" {
                finish(&mut tx, j).await?;
                tx.commit().await?;
                return Ok(());
            }
        }
        if j.kind == "addon.mv.distribute"
            && let Err(e) = workflow::evidence_gate(&mut tx, &o).await
        {
            if matches!(e, Error::PolicyGate(_)) {
                workflow::transition(
                    &mut tx,
                    None,
                    org,
                    id,
                    Status::NEEDS_INFO,
                    "MV_EVIDENCE_INVALID_OR_EXPIRED",
                )
                .await?;
                finish(&mut tx, j).await?;
                tx.commit().await?;
                return Ok(());
            }
            return Err(e);
        }
        let item = WorkItem {
            order_id: id,
            service_code: o.service_code.clone(),
            kind: j.kind.clone(),
            idempotency_key: format!("addon-provider:{}", j.id),
        };
        let outcome = adapter.prepare(&item).await?;
        let (provider, reference) = match outcome {
            AdapterOutcome::Manual { provider } => (provider, None),
            AdapterOutcome::Submitted {
                provider,
                reference,
            } => (provider, Some(reference)),
            AdapterOutcome::Retryable { code } => {
                tx.rollback().await?;
                return operations::fail(pool, j, false, code).await;
            }
            AdapterOutcome::Permanent { code } => {
                tx.rollback().await?;
                return operations::fail(pool, j, true, code).await;
            }
        };
        sqlx::query("INSERT INTO catalog.addon_provider_tasks(id,org_id,addon_order_id,job_id,kind,provider,external_reference,generation) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(job_id,provider) DO NOTHING")
            .bind(Uuid::new_v4()).bind(org).bind(id).bind(j.id).bind(&j.kind).bind(provider).bind(&reference).bind(generation as i32).execute(&mut *tx).await?;
        workflow::audit(&mut tx,None,org,id,if reference.is_some(){"addon.external.submitted"}else{"addon.external.manual_pending"},"PROVIDER_ADAPTER",Value::Null,json!({"job_id":j.id,"kind":j.kind,"provider":provider,"external_reference":reference,"idempotency_key":item.idempotency_key})).await?;
        if o.status == "IN_PROGRESS" {
            workflow::transition(
                &mut tx,
                None,
                org,
                id,
                Status::EXTERNAL_PENDING,
                "PROVIDER_PENDING",
            )
            .await?;
        }
    }
    finish(&mut tx, j).await?;
    tx.commit().await?;
    Ok(())
}
async fn finish(c: &mut PgConnection, j: &Job) -> Result<()> {
    let n=sqlx::query("UPDATE operations.jobs SET status='SUCCEEDED',lock_token=NULL,lease_until=NULL WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp()").bind(j.id).bind(j.token).execute(&mut *c).await?.rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    operations::audit(
        c,
        None,
        None,
        Some(j.id),
        "job.succeeded",
        "ADDON_FENCED",
        Uuid::new_v4(),
    )
    .await
}
pub async fn dead_letter(
    c: &mut PgConnection,
    kind: &str,
    payload: &Value,
    reason: &str,
) -> Result<()> {
    if !kind.starts_with("addon.") {
        return Ok(());
    }
    let uuid = |k: &str| {
        payload[k]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Invalid)
    };
    let org = uuid("org_id")?;
    let id = uuid("order_id")?;
    let o = workflow::load(c, org, id).await?;
    if !o.terminal()
        && o.status != "FAILED"
        && payload["generation"].as_i64() == Some(i64::from(o.dispatch_generation))
    {
        workflow::transition(c, None, org, id, Status::FAILED, reason).await?;
    }
    Ok(())
}
