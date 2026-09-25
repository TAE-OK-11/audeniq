//! F5 distribution execution: partner adapter interface + E-0..E-5 pipeline.
//!
//! BLUEPRINT 16.3/16.4/17.1. The internal immutable `DistributionPackage` is
//! the only input; per-partner mapping lives in the adapter. Capabilities are
//! only ever enabled from the partner's real documentation; the local
//! MockDSP enables all of them.
//!
//! The wire is never inside a PostgreSQL transaction. Local state (delivery
//! jobs, attempts, live bindings) commits in one transaction; the send call
//! is correlated by attempt id + idempotency key. `SENT_UNKNOWN` is never
//! auto-retried: it opens a reconciliation case for explicit human/inquire
//! resolution. Re-sending the same attempt is forbidden.

use crate::error::{Error, Result};
use crate::storage::ObjectStore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use sqlx::{PgConnection, PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Per-partner capability flags (BLUEPRINT 16.3). `capabilities` in
/// `execution.adapter_profiles` is the persisted form; only flags backed by
/// the partner's real documentation may be true.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    pub validate_package: bool,
    pub prepare_transfer: bool,
    pub send_or_publish: bool,
    pub inquire_submission: bool,
    pub parse_ack: bool,
    pub get_release_status: bool,
    pub update_release: bool,
    pub takedown: bool,
    pub receive_royalty_report: bool,
}

impl Capabilities {
    pub fn from_json(v: &Value) -> Self {
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        Self {
            validate_package: b("validate_package"),
            prepare_transfer: b("prepare_transfer"),
            send_or_publish: b("send_or_publish"),
            inquire_submission: b("inquire_submission"),
            parse_ack: b("parse_ack"),
            get_release_status: b("get_release_status"),
            update_release: b("update_release"),
            takedown: b("takedown"),
            receive_royalty_report: b("receive_royalty_report"),
        }
    }
}

/// The frozen package plus the prepared transfer bytes handed to an adapter.
/// Asset bytes themselves are pulled from storage by object key on demand;
/// the manifest carries the integrity pins.
#[derive(Debug, Clone)]
pub struct TransferPackage {
    pub package_id: Uuid,
    pub package_hash: String,
    pub org_id: Uuid,
    pub release_id: Uuid,
    pub ern_xml: Vec<u8>,
    pub files: Vec<TransferFile>,
}

#[derive(Debug, Clone)]
pub struct TransferFile {
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub struct SendContext {
    pub job_id: Uuid,
    pub attempt_id: Uuid,
    pub attempt_no: i32,
    pub idempotency_key: String,
    pub package: TransferPackage,
}

/// Outcome of one wire call. `Timeout`/`Unknown` mean the send state is
/// unknowable: the worker must NOT retry automatically.
#[derive(Debug, Clone)]
pub enum SendOutcome {
    Accepted { partner_message_id: String },
    Rejected { code: String, message: String },
    Timeout,
    Unknown { detail: String },
}

/// Normalized partner webhook event.
#[derive(Debug, Clone)]
pub enum AckEvent {
    Accepted {
        event_id: String,
        partner_message_id: String,
    },
    Rejected {
        event_id: String,
        partner_message_id: String,
        code: String,
    },
    Live {
        event_id: String,
        partner_release_id: String,
    },
    TakedownConfirmed {
        event_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum InquiryOutcome {
    Accepted { partner_message_id: String },
    Rejected { code: String },
    StillUnknown,
    Live { partner_release_id: String },
    TakenDown,
}

/// Common partner adapter interface (BLUEPRINT 16.3). Every method first
/// checks its capability flag; a disabled capability returns `Error::Gated`
/// without touching the wire.
#[async_trait]
pub trait DspAdapter: Send + Sync {
    fn partner_id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;

    fn require(&self, cap: bool, _name: &'static str) -> Result<()> {
        if cap { Ok(()) } else { Err(Error::Gated) }
    }

    async fn validate_package(&self, package: &TransferPackage) -> Result<()>;
    async fn prepare_transfer(&self, package: &TransferPackage) -> Result<Value>;
    async fn send_or_publish(&self, ctx: &SendContext) -> Result<SendOutcome>;
    async fn inquire_submission(&self, partner_message_id: &str) -> Result<InquiryOutcome>;
    async fn parse_ack(&self, payload: &[u8]) -> Result<AckEvent>;
    async fn get_release_status(&self, partner_release_id: &str) -> Result<InquiryOutcome>;
    async fn update_release(&self, ctx: &SendContext, changes: &Value) -> Result<SendOutcome>;
    async fn takedown(&self, ctx: &SendContext) -> Result<SendOutcome>;

    /// Reconcile a send whose response was lost, keyed by the worker's
    /// idempotency key instead of the partner's message id. Default: the
    /// partner offers no such lookup (StillUnknown); adapters whose docs
    /// describe one override it.
    async fn inquire_by_idempotency(&self, _idempotency_key: &str) -> Result<InquiryOutcome> {
        Ok(InquiryOutcome::StillUnknown)
    }
}

/// Authorize this transaction's org for the RLS-protected execution tables.
/// Follows the identifier-ledger convention: the transaction owner sets its
/// org before touching tenant rows.
async fn authorize_org(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, org: Uuid) -> Result<()> {
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// E-0: enqueue one delivery job per eligible DSP for a frozen package.
/// Eligibility now comes from the routing engine (`crate::routing`):
/// each approved DSP resolves to direct > aggregator > upstream, and only
/// ROUTABLE decisions enqueue a job. NO_ROUTE decisions are recorded on the
/// package for F6 contract onboarding instead of failing. CONTRACTED
/// profiles additionally require the contract route (route enabled +
/// endpoint ACTIVE + non-revoked contract revision): delivery_enabled alone
/// is only the operator kill-switch, never the eligibility proof. This is
/// defense in depth behind the Stage 2 eligibility module, which applies
/// the same rule when freezing the route plan.
pub async fn enqueue_delivery_jobs(pool: &PgPool, package_id: Uuid) -> Result<(Vec<Uuid>, Uuid)> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT dp.org_id, dp.id AS package_id, pa.route_plan
         FROM distribution.distribution_packages dp
         JOIN distribution.preparation_artifacts pa ON pa.package_id = dp.id
         WHERE dp.id = $1",
    )
    .bind(package_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let org_id: Uuid = row.get("org_id");
    authorize_org(&mut tx, org_id).await?;
    let route_plan: Value = row.get("route_plan");
    let items = route_plan.as_array().cloned().unwrap_or_default();
    let mut dsp_ids = Vec::new();
    for item in &items {
        if let Some(dsp_id) = item
            .pointer("/scope/dsp_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            dsp_ids.push(dsp_id);
        }
    }
    tx.commit().await?;
    // The routing engine owns profile resolution and the contract-route
    // gate; its verdict is persisted per package for audit and F6.
    let decisions = crate::routing::decide_routes(pool, org_id, &dsp_ids).await?;
    crate::routing::record_route_decisions(pool, org_id, package_id, &decisions).await?;
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org_id).await?;
    let mut job_ids = Vec::new();
    for d in &decisions {
        if !d.routable {
            continue;
        }
        let partner_id = d.partner_id.as_deref().ok_or(Error::Internal)?;
        let job_id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO execution.delivery_jobs(id,org_id,package_id,partner_id)
             VALUES($1,$2,$3,$4) ON CONFLICT(org_id,package_id,partner_id) DO NOTHING RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(package_id)
        .bind(partner_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = job_id {
            job_ids.push(id);
            crate::operations::audit(
                &mut tx,
                None,
                Some(org_id),
                Some(id),
                "delivery.enqueued",
                partner_id,
                Uuid::new_v4(),
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok((job_ids, org_id))
}

pub struct DeliveryJob {
    pub id: Uuid,
    pub token: Uuid,
    pub org_id: Uuid,
    pub package_id: Uuid,
    pub partner_id: String,
    pub attempts: i32,
}

/// E-0 claim: lease one QUEUED delivery job for a partner. Fencing mirrors
/// `operations::claim`: only the lease holder may mutate the job.
pub async fn claim_delivery_job(
    pool: &PgPool,
    org: Uuid,
    partner_id: &str,
    worker: &str,
    lease_seconds: i32,
) -> Result<Option<DeliveryJob>> {
    if !(1..=3600).contains(&lease_seconds) {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let r = sqlx::query(
        "WITH candidate AS (
           SELECT id FROM execution.delivery_jobs
           WHERE partner_id=$1 AND status='QUEUED' AND attempts<max_attempts
           ORDER BY created_at, id FOR UPDATE SKIP LOCKED LIMIT 1
         )
         UPDATE execution.delivery_jobs j
         SET status='LEASED', attempts=attempts+1,
             locked_by=$2, lock_token=$3, lease_until=now()+make_interval(secs=>$4),
             updated_at=now()
         FROM candidate WHERE j.id=candidate.id
         RETURNING j.id, j.lock_token, j.org_id, j.package_id, j.partner_id, j.attempts",
    )
    .bind(partner_id)
    .bind(worker)
    .bind(Uuid::new_v4())
    .bind(lease_seconds as f64)
    .fetch_optional(&mut *tx)
    .await?;
    let job = r.map(|r| DeliveryJob {
        id: r.get("id"),
        token: r.get("lock_token"),
        org_id: r.get("org_id"),
        package_id: r.get("package_id"),
        partner_id: r.get("partner_id"),
        attempts: r.get("attempts"),
    });
    tx.commit().await?;
    Ok(job)
}

async fn job_lease_ok(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job: &DeliveryJob,
) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution.delivery_jobs WHERE id=$1 AND lock_token=$2 AND status IN ('LEASED','SENDING') AND lease_until>clock_timestamp()",
    )
    .bind(job.id)
    .bind(job.token)
    .fetch_one(&mut **tx)
    .await?;
    Ok(n == 1)
}

async fn set_job_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job: &DeliveryJob,
    status: &str,
    last_error: Option<&str>,
) -> Result<()> {
    let n = sqlx::query(
        "UPDATE execution.delivery_jobs SET status=$3, last_error=$4, updated_at=now(),
         lock_token=CASE WHEN $3 IN ('DELIVERED','FAILED','DEAD_LETTER','AWAITING_RECONCILIATION') THEN NULL ELSE lock_token END,
         lease_until=CASE WHEN $3 IN ('DELIVERED','FAILED','DEAD_LETTER','AWAITING_RECONCILIATION') THEN NULL ELSE lease_until END
         WHERE id=$1 AND lock_token=$2 AND lease_until>clock_timestamp()",
    )
    .bind(job.id)
    .bind(job.token)
    .bind(status)
    .bind(last_error)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    crate::operations::audit(
        tx,
        None,
        Some(job.org_id),
        Some(job.id),
        "delivery.status",
        status,
        Uuid::new_v4(),
    )
    .await?;
    Ok(())
}

/// E-1 freshness guard: the frozen package, the release status, the rights
/// epoch and the adapter profile are re-checked under the job lease. Any
/// drift returns the release for correction instead of sending.
async fn freshness_check(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job: &DeliveryJob,
) -> Result<()> {
    let row = sqlx::query(
        "SELECT dp.package_hash, cr.release_id, vp.rights_epoch AS pinned_epoch,
                cr.verification_package_id, r.status AS release_status, r.org_id
         FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         JOIN distribution.verification_packages vp ON vp.id=cr.verification_package_id
         JOIN catalog.releases r ON r.org_id=cr.org_id AND r.id=cr.release_id
         WHERE dp.id=$1",
    )
    .bind(job.package_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<String, _>("release_status") != "READY_FOR_DELIVERY" {
        return Err(Error::PolicyGate("EXECUTION_RELEASE_NOT_READY"));
    }
    let release_id: Uuid = row.get("release_id");
    let org_id: Uuid = row.get("org_id");
    if org_id != job.org_id {
        return Err(Error::PolicyGate("EXECUTION_ORG_MISMATCH"));
    }
    let pinned_epoch: i64 = row.get("pinned_epoch");
    let live_epoch: Option<i64> = sqlx::query_scalar(
        "SELECT epoch FROM rights.rights_epochs WHERE org_id=$1 AND release_id=$2",
    )
    .bind(org_id)
    .bind(release_id)
    .fetch_optional(&mut **tx)
    .await?;
    if live_epoch != Some(pinned_epoch) {
        return Err(Error::PolicyGate("EXECUTION_RIGHTS_EPOCH_DRIFT"));
    }
    // A hold placed after preparation blocks the send. Holds are scoped;
    // a RELEASE hold on this release or any active PARTY hold blocks.
    let held: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM finance.finance_holds
         WHERE org_id=$1 AND active
           AND ((scope_type='RELEASE' AND scope_value=$2) OR scope_type='PARTY'))",
    )
    .bind(org_id)
    .bind(release_id.to_string())
    .fetch_one(&mut **tx)
    .await
    .unwrap_or(false);
    if held {
        return Err(Error::PolicyGate("EXECUTION_HOLD_ACTIVE"));
    }
    let profile: Option<(bool,)> = sqlx::query_as(
        "SELECT delivery_enabled FROM execution.adapter_profiles WHERE partner_id=$1",
    )
    .bind(&job.partner_id)
    .fetch_optional(&mut **tx)
    .await?;
    if profile.map(|(e,)| e) != Some(true) {
        return Err(Error::PolicyGate("EXECUTION_DELIVERY_DISABLED"));
    }
    Ok(())
}

/// E-2 materialize: verify the frozen package bytes and the file manifest
/// against storage before anything goes on the wire.
/// Verify one transfer file against its pinned asset record and storage:
/// the object must exist with the pinned size and content type, and its
/// bytes must hash to the pinned SHA-256. Fails closed on any drift.
async fn verify_file(
    tx: &mut PgConnection,
    storage: &Arc<dyn ObjectStore>,
    org_id: Uuid,
    asset_id: Uuid,
    key: &str,
) -> Result<TransferFile> {
    let pin = sqlx::query(
        "SELECT sha256, size_bytes, content_type FROM catalog.assets WHERE org_id=$1 AND id=$2",
    )
    .bind(org_id)
    .bind(asset_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::PolicyGate("EXECUTION_FILE_PIN_MISSING"))?;
    let sha256: String = pin
        .get::<Option<String>, _>("sha256")
        .ok_or(Error::PolicyGate("EXECUTION_FILE_PIN_MISSING"))?;
    let size_bytes: i64 = pin.get("size_bytes");
    let content_type: String = pin.get("content_type");
    if size_bytes > crate::preflight::MAX_PREFLIGHT_ASSET_BYTES {
        return Err(Error::PolicyGate("EXECUTION_FILE_TOO_LARGE"));
    }
    let head = storage
        .head(key)
        .await
        .map_err(|_| Error::Storage)?
        .ok_or(Error::Storage)?;
    if head.size != size_bytes || head.content_type != content_type {
        return Err(Error::PolicyGate("EXECUTION_FILE_DRIFT"));
    }
    let bytes = storage.get(key).await.map_err(|_| Error::Storage)?;
    if hex::encode(sha2::Sha256::digest(&bytes)) != sha256 {
        return Err(Error::PolicyGate("EXECUTION_FILE_TAMPERED"));
    }
    Ok(TransferFile {
        object_key: key.to_string(),
        sha256,
        size_bytes,
        content_type,
    })
}

async fn materialize(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    storage: &Arc<dyn ObjectStore>,
    job: &DeliveryJob,
) -> Result<TransferPackage> {
    let row = sqlx::query(
        "SELECT dp.package_hash, dp.body, cr.org_id, cr.release_id,
                pa.ern_sha256, pa.ern_xml, pa.preflight_report
         FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         JOIN distribution.preparation_artifacts pa ON pa.package_id=dp.id
         WHERE dp.id=$1",
    )
    .bind(job.package_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(Error::NotFound)?;
    let body: Value = row.get("body");
    let package_hash: String = row.get("package_hash");
    // The frozen body must hash to the stored package hash.
    let recomputed = hex::encode(sha2::Sha256::digest(
        serde_json::to_string(&body).unwrap_or_default().as_bytes(),
    ));
    if recomputed != package_hash {
        return Err(Error::PolicyGate("EXECUTION_PACKAGE_TAMPERED"));
    }
    let preflight: Value = row.get("preflight_report");
    for check in ["xml", "metadata", "files", "rights"] {
        if preflight.get(check).and_then(Value::as_str) != Some("Pass") {
            return Err(Error::PolicyGate("EXECUTION_PREFLIGHT_NOT_PASS"));
        }
    }
    // Rebuild the transfer manifest from the canonical release: every file
    // must still exist in storage with the pinned size/content type.
    let canonical_id: Uuid = sqlx::query_scalar(
        "SELECT canonical_release_id FROM distribution.distribution_packages WHERE id=$1",
    )
    .bind(job.package_id)
    .fetch_one(&mut **tx)
    .await?;
    let snapshot: Value =
        sqlx::query_scalar("SELECT body FROM distribution.canonical_releases WHERE id=$1")
            .bind(canonical_id)
            .fetch_one(&mut **tx)
            .await?;
    let mut files = Vec::new();
    // Every file is verified against its pinned catalog.assets record and
    // then against storage bytes: existence, size, content type and SHA-256
    // must all match the pin. This mirrors the F4 preflight content check;
    // E-2 re-verifies because bytes may have changed between preparation
    // and send. Anything missing or mismatched fails closed.
    let org_id: Uuid = row.get("org_id");
    if let Some(tracks) = snapshot.get("tracks").and_then(Value::as_array) {
        for t in tracks {
            let key = t
                .get("asset_object_key")
                .and_then(Value::as_str)
                .ok_or(Error::PolicyGate("EXECUTION_FILE_REF_MISSING"))?;
            let asset_id: Uuid = t
                .get("asset_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(Error::PolicyGate("EXECUTION_FILE_REF_MISSING"))?;
            files.push(verify_file(tx, storage, org_id, asset_id, key).await?);
        }
    }
    if let Some(art) = snapshot.get("artwork") {
        let key = art
            .get("object_key")
            .and_then(Value::as_str)
            .ok_or(Error::PolicyGate("EXECUTION_FILE_REF_MISSING"))?;
        let asset_id: Uuid = art
            .get("asset_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::PolicyGate("EXECUTION_FILE_REF_MISSING"))?;
        files.push(verify_file(tx, storage, org_id, asset_id, key).await?);
    }
    // Transfer document routing. The partner-specific DDEX interchange
    // message persisted for this package+DSP is the real interchange
    // artifact and wins when present. The synthetic preparation envelope is
    // only a fallback for the mock transport; a real transport with no DDEX
    // message fails closed instead of silently sending the synthetic bytes.
    let profile: Option<(Option<Uuid>, String, String)> = sqlx::query_as(
        "SELECT dsp_id, transport, activation_kind FROM execution.adapter_profiles WHERE partner_id=$1",
    )
    .bind(&job.partner_id)
    .fetch_optional(&mut **tx)
    .await?;
    let (dsp_id, transport, activation_kind) =
        profile.unwrap_or((None, "mock".to_string(), "MOCK".to_string()));
    let ddex: Option<(String, String)> = match dsp_id {
        Some(dsp) => sqlx::query_as(
            "SELECT ern_xml, ern_sha256 FROM distribution.ddex_messages WHERE package_id=$1 AND dsp_id=$2",
        )
        .bind(job.package_id)
        .bind(dsp)
        .fetch_optional(&mut **tx)
        .await?,
        None => None,
    };
    let (ern_xml, ern_sha256): (String, String) = match ddex {
        Some((xml, sha)) => (xml, sha),
        // A commercial partner never receives the synthetic preparation
        // envelope: without its own DDEX interchange message the send fails
        // closed before any wire call. The mock transport keeps the
        // synthetic fallback for local testing only.
        None if transport != "mock" || activation_kind == "CONTRACTED" => {
            return Err(Error::PolicyGate("EXECUTION_DDEX_MESSAGE_MISSING"));
        }
        None => (
            row.try_get("ern_xml")
                .ok()
                .flatten()
                .ok_or(Error::PolicyGate("EXECUTION_ERN_MISSING"))?,
            row.get("ern_sha256"),
        ),
    };
    // The transfer document is the ERN XML frozen at preparation time, not a
    // synthetic comment. Its bytes are verified against the stored hash;
    // a missing or tampered document fails closed before any wire call.
    if hex::encode(sha2::Sha256::digest(ern_xml.as_bytes())) != ern_sha256 {
        return Err(Error::PolicyGate("EXECUTION_ERN_TAMPERED"));
    }
    let ern_xml_bytes = ern_xml.into_bytes();
    Ok(TransferPackage {
        package_id: job.package_id,
        package_hash,
        org_id: row.get("org_id"),
        release_id: row.get("release_id"),
        ern_xml: ern_xml_bytes,
        files,
    })
}

/// E-3 send: exactly one wire call per attempt row. The idempotency key is
/// inserted BEFORE the call; a duplicate key can never reach the wire twice.
pub async fn run_delivery(
    pool: &PgPool,
    storage: &Arc<dyn ObjectStore>,
    adapter: &dyn DspAdapter,
    job: &DeliveryJob,
) -> Result<String> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, job.org_id).await?;
    if !job_lease_ok(&mut tx, job).await? {
        return Err(Error::Conflict);
    }
    if adapter.partner_id() != job.partner_id {
        return Err(Error::PolicyGate("EXECUTION_PARTNER_MISMATCH"));
    }
    // E-1. A failed check poisons the Postgres transaction (any error aborts
    // it), so the failure bookkeeping below runs in a FRESH transaction via
    // fail_job — reusing `tx` would mask the real error with 25P02.
    if let Err(e) = freshness_check(&mut tx, job).await {
        let msg = format!("{e:?}");
        tx.rollback().await?;
        fail_job(
            pool,
            job,
            "FAILED",
            &msg,
            Some("PARTNER_REJECTED"),
            Some(json!({"stage":"E-1","error":msg})),
        )
        .await?;
        return Err(e);
    }
    // E-2. Same poisoned-transaction rule as E-1.
    let package = match materialize(&mut tx, storage, job).await {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("{e:?}");
            tx.rollback().await?;
            fail_job(pool, job, "FAILED", &msg, None, None).await?;
            return Err(e);
        }
    };
    adapter.require(adapter.capabilities().send_or_publish, "send_or_publish")?;
    adapter.validate_package(&package).await?;
    let _prepared = adapter.prepare_transfer(&package).await?;

    // E-3. The attempt row (with its idempotency key) commits before the
    // wire call so a crash between call and record cannot double-send:
    // recovery replays from the recorded attempt, never a new key.
    // Crash recovery: a previous run may have recorded the attempt (with its
    // idempotency key) and then died before/while calling the wire. The
    // send state is unknowable, so we must NOT send again — reconcile the
    // existing attempt instead. This is the zero-duplicate-send guarantee.
    let unresolved: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM execution.delivery_attempts
         WHERE job_id=$1 AND outcome IN ('IN_FLIGHT','TIMEOUT','UNKNOWN')
         ORDER BY attempt_no DESC LIMIT 1",
    )
    .bind(job.id)
    .fetch_optional(&mut *tx)
    .await?;
    if unresolved.is_some() {
        set_job_status(
            &mut tx,
            job,
            "AWAITING_RECONCILIATION",
            Some("SENT_UNKNOWN"),
        )
        .await?;
        open_case(
            &mut tx,
            job,
            "SENT_UNKNOWN",
            &json!({"note": "unresolved attempt from previous run"}),
        )
        .await?;
        tx.commit().await?;
        return Ok("AWAITING_RECONCILIATION".to_string());
    }

    let attempt_id = Uuid::new_v4();
    let attempt_no: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(attempt_no),0)+1 FROM execution.delivery_attempts WHERE job_id=$1",
    )
    .bind(job.id)
    .fetch_one(&mut *tx)
    .await?;
    let idempotency_key = format!("delivery:{}:{attempt_no}", job.id);
    let request_sha256 = hex::encode(sha2::Sha256::digest(&package.ern_xml));
    let inserted = sqlx::query_scalar::<_, i32>(
        "INSERT INTO execution.delivery_attempts(id,org_id,job_id,attempt_no,idempotency_key,request_sha256,outcome)
         VALUES($1,$2,$3,$4,$5,$6,'IN_FLIGHT') ON CONFLICT DO NOTHING RETURNING 1",
    )
    .bind(attempt_id)
    .bind(job.org_id)
    .bind(job.id)
    .bind(attempt_no)
    .bind(&idempotency_key)
    .bind(&request_sha256)
    .fetch_optional(&mut *tx)
    .await?;
    if inserted.is_none() {
        // Same key already sent: this is a replay, not a new send.
        tx.rollback().await?;
        return Err(Error::Conflict);
    }
    set_job_status(&mut tx, job, "SENDING", None).await?;
    tx.commit().await?;

    // The wire call happens OUTSIDE the transaction (BLUEPRINT 16.4).
    let ctx = SendContext {
        job_id: job.id,
        attempt_id,
        attempt_no,
        idempotency_key: idempotency_key.clone(),
        package,
    };
    let outcome = adapter.send_or_publish(&ctx).await;

    // Record the outcome. Every branch is terminal for this attempt; only
    // Accepted advances the job, and Unknown/Timeout never auto-retry.
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, job.org_id).await?;
    match outcome {
        Ok(SendOutcome::Accepted { partner_message_id }) => {
            sqlx::query(
                "UPDATE execution.delivery_attempts SET outcome='ACCEPTED', partner_message_id=$2, response=$3 WHERE id=$1",
            )
            .bind(attempt_id)
            .bind(&partner_message_id)
            .bind(json!({"partner_message_id": partner_message_id}))
            .execute(&mut *tx)
            .await?;
            set_job_status(&mut tx, job, "DELIVERED", None).await?;
            upsert_live_binding(&mut tx, job, "INGESTING", None).await?;
            tx.commit().await?;
            Ok("DELIVERED".to_string())
        }
        Ok(SendOutcome::Rejected { code, message }) => {
            sqlx::query(
                "UPDATE execution.delivery_attempts SET outcome='REJECTED', response=$2 WHERE id=$1",
            )
            .bind(attempt_id)
            .bind(json!({"code": code, "message": message}))
            .execute(&mut *tx)
            .await?;
            set_job_status(
                &mut tx,
                job,
                "FAILED",
                Some(&format!("PARTNER_REJECTED:{code}")),
            )
            .await?;
            open_case(&mut tx, job, "PARTNER_REJECTED", &json!({"code": code})).await?;
            tx.commit().await?;
            Ok("FAILED".to_string())
        }
        Ok(SendOutcome::Timeout) | Ok(SendOutcome::Unknown { .. }) | Err(_) => {
            let label = match &outcome {
                Ok(SendOutcome::Timeout) => "TIMEOUT",
                _ => "UNKNOWN",
            };
            sqlx::query(
                "UPDATE execution.delivery_attempts SET outcome=$2, response=$3 WHERE id=$1",
            )
            .bind(attempt_id)
            .bind(label)
            .bind(json!({"note": "send state unknowable; no auto-retry"}))
            .execute(&mut *tx)
            .await?;
            // Never auto-retry: park for explicit reconciliation.
            set_job_status(
                &mut tx,
                job,
                "AWAITING_RECONCILIATION",
                Some("SENT_UNKNOWN"),
            )
            .await?;
            open_case(
                &mut tx,
                job,
                "SENT_UNKNOWN",
                &json!({"attempt_id": attempt_id}),
            )
            .await?;
            tx.commit().await?;
            Ok("AWAITING_RECONCILIATION".to_string())
        }
    }
}

async fn upsert_live_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job: &DeliveryJob,
    live_status: &str,
    partner_release_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO execution.live_bindings(id,org_id,package_id,partner_id,live_status,partner_release_id,last_checked_at)
         VALUES($1,$2,$3,$4,$5,$6,now())
         ON CONFLICT(org_id,package_id,partner_id) DO UPDATE
         SET live_status=EXCLUDED.live_status, partner_release_id=COALESCE(EXCLUDED.partner_release_id, execution.live_bindings.partner_release_id),
             last_checked_at=now(), updated_at=now()",
    )
    .bind(Uuid::new_v4())
    .bind(job.org_id)
    .bind(job.package_id)
    .bind(&job.partner_id)
    .bind(live_status)
    .bind(partner_release_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn open_case(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job: &DeliveryJob,
    kind: &str,
    detail: &Value,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO execution.reconciliation_cases(id,org_id,job_id,kind,detail)
         VALUES($1,$2,$3,$4,$5) ON CONFLICT(job_id,kind,status) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(job.org_id)
    .bind(job.id)
    .bind(kind)
    .bind(detail)
    .execute(&mut **tx)
    .await?;
    crate::operations::audit(
        tx,
        None,
        Some(job.org_id),
        Some(job.id),
        "delivery.reconcile_case",
        kind,
        Uuid::new_v4(),
    )
    .await?;
    Ok(())
}

/// Record a terminal job failure (and optionally a reconciliation case) in a
/// fresh transaction. Used when the caller's transaction may be poisoned: in
/// Postgres any failed statement aborts the whole transaction, so failure
/// bookkeeping must never reuse it.
async fn fail_job(
    pool: &PgPool,
    job: &DeliveryJob,
    status: &str,
    err: &str,
    case_kind: Option<&str>,
    case_detail: Option<Value>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, job.org_id).await?;
    set_job_status(&mut tx, job, status, Some(err)).await?;
    if let (Some(kind), Some(detail)) = (case_kind, case_detail) {
        open_case(&mut tx, job, kind, &detail).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Webhook ACK ingestion. Deduplicated on the partner event id: a duplicate
/// delivery of the same event is acknowledged without re-applying state.
/// The caller supplies the org: the webhook receiver resolves it from its
/// per-org partner endpoint configuration (F6 wires the real mapping).
pub async fn ingest_ack(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    payload: &[u8],
) -> Result<String> {
    adapter.require(adapter.capabilities().parse_ack, "parse_ack")?;
    let event = adapter.parse_ack(payload).await?;
    let (event_id, outcome, partner_message_id, partner_release_id, extra) = match &event {
        AckEvent::Accepted {
            event_id,
            partner_message_id,
        } => (
            event_id.clone(),
            "ACCEPTED",
            Some(partner_message_id.clone()),
            None,
            json!({}),
        ),
        AckEvent::Rejected {
            event_id,
            partner_message_id,
            code,
        } => (
            event_id.clone(),
            "REJECTED",
            Some(partner_message_id.clone()),
            None,
            json!({"code": code}),
        ),
        AckEvent::Live {
            event_id,
            partner_release_id,
        } => (
            event_id.clone(),
            "LIVE",
            None,
            Some(partner_release_id.clone()),
            json!({}),
        ),
        AckEvent::TakedownConfirmed { event_id } => {
            (event_id.clone(), "TAKEN_DOWN", None, None, json!({}))
        }
    };
    // Dedupe: the same partner event must never apply twice.
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let seen: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.delivery_attempts WHERE response->>'ack_event_id' = $1)",
    )
    .bind(&event_id)
    .fetch_one(&mut *tx)
    .await?;
    if seen {
        tx.rollback().await?;
        return Ok("DUPLICATE_IGNORED".to_string());
    }
    // Correlate to the attempt by partner message id (or the live binding).
    let mut applied = false;
    if let Some(pmid) = &partner_message_id {
        let n = sqlx::query(
            "UPDATE execution.delivery_attempts SET response = response || $2 || $3 WHERE partner_message_id=$1",
        )
        .bind(pmid)
        .bind(json!({"ack_event_id": event_id, "ack_outcome": outcome}))
        .bind(&extra)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        applied = n > 0;
        if applied && outcome == "ACCEPTED" {
            sqlx::query(
                "UPDATE execution.delivery_jobs j SET status='DELIVERED', updated_at=now()
                 FROM execution.delivery_attempts a
                 WHERE a.partner_message_id=$1 AND a.job_id=j.id AND j.status IN ('SENDING','AWAITING_RECONCILIATION')",
            )
            .bind(pmid)
            .execute(&mut *tx)
            .await?;
        }
        if applied && outcome == "REJECTED" {
            sqlx::query(
                "UPDATE execution.delivery_jobs j SET status='FAILED', last_error='PARTNER_WEBHOOK_REJECTED', updated_at=now()
                 FROM execution.delivery_attempts a
                 WHERE a.partner_message_id=$1 AND a.job_id=j.id",
            )
            .bind(pmid)
            .execute(&mut *tx)
            .await?;
        }
    }
    if let Some(prid) = &partner_release_id {
        sqlx::query(
            "UPDATE execution.live_bindings SET live_status='LIVE', partner_release_id=$1, last_checked_at=now(), updated_at=now()
             WHERE partner_release_id=$1 OR (live_status IN ('UNKNOWN','INGESTING') AND partner_id=$2)",
        )
        .bind(prid)
        .bind(adapter.partner_id())
        .execute(&mut *tx)
        .await?;
        applied = true;
    }
    if outcome == "TAKEN_DOWN" {
        sqlx::query(
            "UPDATE execution.live_bindings SET live_status='TAKEN_DOWN', last_checked_at=now(), updated_at=now()
             WHERE partner_id=$1 AND live_status='TAKEDOWN_REQUESTED'",
        )
        .bind(adapter.partner_id())
        .execute(&mut *tx)
        .await?;
        applied = true;
    }
    tx.commit().await?;
    Ok(if applied {
        "APPLIED".to_string()
    } else {
        "UNMATCHED".to_string()
    })
}

/// E-4: poll the partner for ingest/live state and refresh live bindings.
pub async fn poll_live(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    package_id: Uuid,
    partner_id: &str,
) -> Result<String> {
    adapter.require(
        adapter.capabilities().get_release_status,
        "get_release_status",
    )?;
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let binding = sqlx::query(
        "SELECT b.id, b.partner_release_id, j.org_id, j.id AS job_id
         FROM execution.live_bindings b
         JOIN execution.delivery_jobs j ON j.package_id=b.package_id AND j.partner_id=b.partner_id
         WHERE b.package_id=$1 AND b.partner_id=$2",
    )
    .bind(package_id)
    .bind(partner_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let partner_release_id: Option<String> = binding.get("partner_release_id");
    tx.commit().await?;

    let outcome = match &partner_release_id {
        Some(prid) => adapter.get_release_status(prid).await?,
        None => {
            // No partner release id yet: ask about the latest accepted attempt.
            // RLS applies: this lookup needs the org authorized like the rest.
            let mut atx = pool.begin().await?;
            authorize_org(&mut atx, org).await?;
            let pmid: Option<String> = sqlx::query_scalar(
                "SELECT a.partner_message_id FROM execution.delivery_attempts a
                 JOIN execution.delivery_jobs j ON j.id=a.job_id
                 WHERE j.package_id=$1 AND j.partner_id=$2 AND a.partner_message_id IS NOT NULL
                 ORDER BY a.attempt_no DESC LIMIT 1",
            )
            .bind(package_id)
            .bind(partner_id)
            .fetch_optional(&mut *atx)
            .await?
            .flatten();
            atx.commit().await?;
            match pmid {
                Some(id) => adapter.inquire_submission(&id).await?,
                None => return Ok("NO_ATTEMPT".to_string()),
            }
        }
    };
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let status = match outcome {
        InquiryOutcome::Live { partner_release_id } => {
            sqlx::query(
                "UPDATE execution.live_bindings SET live_status='LIVE', partner_release_id=$3, last_checked_at=now(), updated_at=now()
                 WHERE package_id=$1 AND partner_id=$2",
            )
            .bind(package_id)
            .bind(partner_id)
            .bind(&partner_release_id)
            .execute(&mut *tx)
            .await?;
            "LIVE"
        }
        InquiryOutcome::TakenDown => {
            sqlx::query(
                "UPDATE execution.live_bindings SET live_status='TAKEN_DOWN', last_checked_at=now(), updated_at=now()
                 WHERE package_id=$1 AND partner_id=$2",
            )
            .bind(package_id)
            .bind(partner_id)
            .execute(&mut *tx)
            .await?;
            "TAKEN_DOWN"
        }
        InquiryOutcome::Rejected { code } => {
            sqlx::query(
                "UPDATE execution.delivery_jobs SET status='FAILED', last_error=$3, updated_at=now()
                 WHERE package_id=$1 AND partner_id=$2",
            )
            .bind(package_id)
            .bind(partner_id)
            .bind(format!("PARTNER_POLL_REJECTED:{code}"))
            .execute(&mut *tx)
            .await?;
            "REJECTED"
        }
        InquiryOutcome::Accepted { .. } | InquiryOutcome::StillUnknown => {
            sqlx::query(
                "UPDATE execution.live_bindings SET live_status='INGESTING', last_checked_at=now(), updated_at=now()
                 WHERE package_id=$1 AND partner_id=$2",
            )
            .bind(package_id)
            .bind(partner_id)
            .execute(&mut *tx)
            .await?;
            "INGESTING"
        }
    };
    tx.commit().await?;
    Ok(status.to_string())
}

/// E-5: open reconciliation cases for jobs that need human attention:
/// missing ACKs past the deadline, SENT_UNKNOWN attempts, overdue live.
/// Sweeps per org: the reconciler authorizes each org before touching its
/// RLS-protected rows.
pub async fn reconcile(pool: &PgPool, ack_deadline_secs: i64) -> Result<usize> {
    // identity.orgs carries no RLS (tenant isolation there is by grant, not
    // policy), so the sweeper lists orgs directly and authorizes each one
    // before touching its RLS-protected execution rows in reconcile_org.
    let orgs: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM identity.orgs")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let mut total = 0;
    for org in orgs {
        total += reconcile_org(pool, org, ack_deadline_secs).await?;
    }
    Ok(total)
}

async fn reconcile_org(pool: &PgPool, org: Uuid, ack_deadline_secs: i64) -> Result<usize> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    // Missing ACK: DELIVERED-by-wire but no ack event recorded past deadline.
    let missing = sqlx::query(
        "SELECT j.id, j.org_id FROM execution.delivery_jobs j
         WHERE j.status='DELIVERED' AND NOT EXISTS (
           SELECT 1 FROM execution.delivery_attempts a
           WHERE a.job_id=j.id AND a.response ? 'ack_event_id'
         )
         AND j.updated_at < now() - make_interval(secs=>$1)",
    )
    .bind(ack_deadline_secs as f64)
    .fetch_all(&mut *tx)
    .await?;
    let mut opened = 0;
    for r in &missing {
        let job = DeliveryJobRef {
            id: r.get("id"),
            org_id: r.get("org_id"),
        };
        if open_case_for(&mut tx, &job, "MISSING_ACK").await? {
            opened += 1;
        }
    }
    // Overdue live: accepted but never went live past deadline.
    let overdue = sqlx::query(
        "SELECT j.id, j.org_id FROM execution.delivery_jobs j
         JOIN execution.live_bindings b ON b.package_id=j.package_id AND b.partner_id=j.partner_id
         WHERE j.status='DELIVERED' AND b.live_status IN ('UNKNOWN','INGESTING')
         AND b.last_checked_at < now() - make_interval(secs=>$1)",
    )
    .bind(ack_deadline_secs as f64)
    .fetch_all(&mut *tx)
    .await?;
    for r in &overdue {
        let job = DeliveryJobRef {
            id: r.get("id"),
            org_id: r.get("org_id"),
        };
        if open_case_for(&mut tx, &job, "OVERDUE_LIVE").await? {
            opened += 1;
        }
    }
    tx.commit().await?;
    Ok(opened)
}

struct DeliveryJobRef {
    id: Uuid,
    org_id: Uuid,
}

async fn open_case_for(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job: &DeliveryJobRef,
    kind: &str,
) -> Result<bool> {
    let n = sqlx::query(
        "INSERT INTO execution.reconciliation_cases(id,org_id,job_id,kind)
         VALUES($1,$2,$3,$4) ON CONFLICT(job_id,kind,status) DO NOTHING RETURNING 1",
    )
    .bind(Uuid::new_v4())
    .bind(job.org_id)
    .bind(job.id)
    .bind(kind)
    .fetch_optional(&mut **tx)
    .await?
    .is_some();
    Ok(n)
}

/// Resolve a SENT_UNKNOWN attempt by explicit inquiry. This is the ONLY path
/// that may follow an unknown send: a human-triggered (or reconciler-driven)
/// status check, never an automatic re-send of the same attempt.
pub async fn resolve_unknown(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    job_id: Uuid,
) -> Result<String> {
    adapter.require(
        adapter.capabilities().inquire_submission,
        "inquire_submission",
    )?;
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let row = sqlx::query(
        "SELECT a.id AS attempt_id, a.partner_message_id, a.idempotency_key, j.org_id, j.partner_id, j.package_id
         FROM execution.delivery_attempts a JOIN execution.delivery_jobs j ON j.id=a.job_id
         WHERE a.job_id=$1 AND a.outcome IN ('IN_FLIGHT','TIMEOUT','UNKNOWN')
         ORDER BY a.attempt_no DESC LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let attempt_id: Uuid = row.get("attempt_id");
    let pmid: Option<String> = row.get("partner_message_id");
    let idem_key: String = row.get("idempotency_key");
    let org_id: Uuid = row.get("org_id");
    tx.commit().await?;

    // With a partner message id we ask about the submission; without one
    // (response lost) we correlate by our own idempotency key. Either way
    // this is an explicit inquiry, never a re-send.
    let outcome = match &pmid {
        Some(id) => adapter.inquire_submission(id).await?,
        None => adapter.inquire_by_idempotency(&idem_key).await?,
    };
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let status = match outcome {
        InquiryOutcome::Accepted { partner_message_id } => {
            sqlx::query(
                "UPDATE execution.delivery_attempts SET outcome='ACCEPTED', partner_message_id=$2 WHERE id=$1",
            )
            .bind(attempt_id)
            .bind(&partner_message_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE execution.delivery_jobs SET status='DELIVERED', updated_at=now() WHERE id=$1")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
            "DELIVERED"
        }
        InquiryOutcome::Live { partner_release_id } => {
            sqlx::query("UPDATE execution.delivery_jobs SET status='DELIVERED', updated_at=now() WHERE id=$1")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
            let job = DeliveryJob {
                id: job_id,
                token: Uuid::nil(),
                org_id,
                package_id: row.get("package_id"),
                partner_id: row.get("partner_id"),
                attempts: 0,
            };
            upsert_live_binding(&mut tx, &job, "LIVE", Some(&partner_release_id)).await?;
            "LIVE"
        }
        InquiryOutcome::Rejected { code } => {
            sqlx::query("UPDATE execution.delivery_jobs SET status='FAILED', last_error=$2, updated_at=now() WHERE id=$1")
                .bind(job_id)
                .bind(format!("INQUIRY_REJECTED:{code}"))
                .execute(&mut *tx)
                .await?;
            "FAILED"
        }
        InquiryOutcome::StillUnknown | InquiryOutcome::TakenDown => "STILL_UNKNOWN",
    };
    // The case stays open until a human resolves it; the inquiry only
    // records what the partner actually said.
    tx.commit().await?;
    Ok(status.to_string())
}

/// Partner metadata update with capability gating. Recorded as its own
/// wire call on the mock; real adapters map it to the partner's update
/// choreography (F6).
pub async fn update_release(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    package_id: Uuid,
    partner_id: &str,
    changes: &Value,
) -> Result<String> {
    adapter.require(adapter.capabilities().update_release, "update_release")?;
    let mut tx0 = pool.begin().await?;
    authorize_org(&mut tx0, org).await?;
    let row = sqlx::query(
        "SELECT j.id AS job_id, j.org_id
         FROM execution.delivery_jobs j
         WHERE j.package_id=$1 AND j.partner_id=$2 AND j.status='DELIVERED'",
    )
    .bind(package_id)
    .bind(partner_id)
    .fetch_optional(&mut *tx0)
    .await?
    .ok_or(Error::NotFound)?;
    tx0.commit().await?;
    let job_id: Uuid = row.get("job_id");
    let ctx = SendContext {
        job_id,
        attempt_id: Uuid::new_v4(),
        attempt_no: 0,
        idempotency_key: format!("update:{job_id}"),
        package: TransferPackage {
            package_id,
            package_hash: String::new(),
            org_id: row.get("org_id"),
            release_id: Uuid::nil(),
            ern_xml: Vec::new(),
            files: Vec::new(),
        },
    };
    match adapter.update_release(&ctx, changes).await? {
        SendOutcome::Accepted { .. } => Ok("UPDATE_ACCEPTED".to_string()),
        SendOutcome::Rejected { .. } => Err(Error::PolicyGate("UPDATE_REJECTED")),
        SendOutcome::Timeout | SendOutcome::Unknown { .. } => {
            Err(Error::PolicyGate("UPDATE_UNKNOWN"))
        }
    }
}

/// Partner takedown with capability gating. The live binding moves to
/// TAKEDOWN_REQUESTED; TAKEN_DOWN is confirmed by webhook or poll.
pub async fn takedown_release(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    package_id: Uuid,
    partner_id: &str,
) -> Result<String> {
    adapter.require(adapter.capabilities().takedown, "takedown")?;
    let mut tx0 = pool.begin().await?;
    authorize_org(&mut tx0, org).await?;
    let row = sqlx::query(
        "SELECT j.id AS job_id, j.org_id, b.partner_release_id
         FROM execution.delivery_jobs j
         LEFT JOIN execution.live_bindings b ON b.package_id=j.package_id AND b.partner_id=j.partner_id
         WHERE j.package_id=$1 AND j.partner_id=$2",
    )
    .bind(package_id)
    .bind(partner_id)
    .fetch_optional(&mut *tx0)
    .await?
    .ok_or(Error::NotFound)?;
    tx0.commit().await?;
    let job_id: Uuid = row.get("job_id");
    let ctx = SendContext {
        job_id,
        attempt_id: Uuid::new_v4(),
        attempt_no: 0,
        idempotency_key: format!("takedown:{job_id}"),
        package: TransferPackage {
            package_id,
            package_hash: String::new(),
            org_id: row.get("org_id"),
            release_id: Uuid::nil(),
            ern_xml: Vec::new(),
            files: Vec::new(),
        },
    };
    let outcome = adapter.takedown(&ctx).await?;
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    match outcome {
        SendOutcome::Accepted { .. } => {
            sqlx::query(
                "UPDATE execution.live_bindings SET live_status='TAKEDOWN_REQUESTED', last_checked_at=now(), updated_at=now()
                 WHERE package_id=$1 AND partner_id=$2",
            )
            .bind(package_id)
            .bind(partner_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok("TAKEDOWN_REQUESTED".to_string())
        }
        SendOutcome::Rejected { .. } => {
            tx.rollback().await?;
            Err(Error::PolicyGate("TAKEDOWN_REJECTED"))
        }
        SendOutcome::Timeout | SendOutcome::Unknown { .. } => {
            tx.rollback().await?;
            Err(Error::PolicyGate("TAKEDOWN_UNKNOWN"))
        }
    }
}

/// Lease a specific delivery job by id (used by the operations dispatcher).
/// Only QUEUED jobs (or jobs whose lease expired) can be leased.
pub async fn lease_delivery_job(
    pool: &PgPool,
    job_id: Uuid,
    org: Uuid,
    worker: &str,
    lease_seconds: i32,
) -> Result<Option<DeliveryJob>> {
    if !(1..=3600).contains(&lease_seconds) {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let r = sqlx::query(
        "UPDATE execution.delivery_jobs
         SET status='LEASED', attempts=attempts+1,
             locked_by=$2, lock_token=$3, lease_until=now()+make_interval(secs=>$4),
             updated_at=now()
         WHERE id=$1 AND attempts<max_attempts
           AND (status='QUEUED' OR (status IN ('LEASED','SENDING') AND lease_until<=now()))
         RETURNING id, lock_token, org_id, package_id, partner_id, attempts",
    )
    .bind(job_id)
    .bind(worker)
    .bind(Uuid::new_v4())
    .bind(lease_seconds as f64)
    .fetch_optional(&mut *tx)
    .await?;
    let job = r.map(|r| DeliveryJob {
        id: r.get("id"),
        token: r.get("lock_token"),
        org_id: r.get("org_id"),
        package_id: r.get("package_id"),
        partner_id: r.get("partner_id"),
        attempts: r.get("attempts"),
    });
    tx.commit().await?;
    Ok(job)
}

/// RLS-safe status read for the operations dispatcher: the caller passes the
/// org from the job payload and this authorizes before touching the
/// RLS-protected table. Returns None when the job does not exist.
pub async fn delivery_job_status(pool: &PgPool, org: Uuid, job_id: Uuid) -> Result<Option<String>> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM execution.delivery_jobs WHERE id=$1")
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(status)
}

/// Minimal adapter registry. F5 ships exactly one partner: the local
/// MockDSP. Real partners (F6/F9) register here behind their own profiles.
pub struct AdapterRegistry {
    adapters: HashMap<String, Arc<dyn DspAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(&mut self, adapter: Arc<dyn DspAdapter>) {
        self.adapters
            .insert(adapter.partner_id().to_string(), adapter);
    }

    pub fn get(&self, partner_id: &str) -> Option<Arc<dyn DspAdapter>> {
        self.adapters.get(partner_id).cloned()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
