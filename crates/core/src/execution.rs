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
    /// Release UPC from the frozen snapshot (file-drop folder names).
    pub upc: Option<String>,
    /// Full release metadata for partner-specific feeds (Korean services,
    /// REST APIs). None when it cannot be rebuilt (legacy fixtures); those
    /// adapters then fail closed before any wire call.
    pub prepared: Option<Arc<crate::preparation_model::PreparedRelease>>,
    /// Genre and label from the frozen application (partner feeds).
    pub genre: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    Audio,
    Artwork,
}

#[derive(Debug, Clone)]
pub struct TransferFile {
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub content_type: String,
    /// Name the partner receives, exactly as the ERN references it.
    pub delivery_name: String,
    pub role: FileRole,
}

#[derive(Debug, Clone)]
pub struct SendContext {
    pub job_id: Uuid,
    pub attempt_id: Uuid,
    pub attempt_no: i32,
    pub idempotency_key: String,
    /// Partner-side id chosen before the wire call (a DDEX batch folder).
    /// Recorded on the attempt row first, so a lost response can still be
    /// reconciled by asking the partner about this exact id.
    pub planned_message_id: Option<String>,
    pub package: TransferPackage,
}

/// Outcome of one wire call. `Timeout`/`Unknown` mean the send state is
/// unknowable: the worker must NOT retry automatically.
#[derive(Debug, Clone)]
pub enum SendOutcome {
    Accepted {
        partner_message_id: String,
    },
    Rejected {
        code: String,
        message: String,
    },
    Timeout,
    Unknown {
        detail: String,
    },
    /// The partner refused the call before processing it (e.g. HTTP 503 or
    /// a connection refused before any byte was accepted): nothing was
    /// created partner-side, so a new attempt is safe.
    Unavailable {
        detail: String,
    },
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
        /// Our submission the partner release came from, when the partner
        /// says so (correlates the event to one delivery).
        partner_message_id: Option<String>,
    },
    TakedownConfirmed {
        event_id: String,
        partner_release_id: Option<String>,
        partner_message_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// The partner message id this send will use, when the adapter picks it
    /// itself (file drops). Default: the partner assigns it (None).
    fn plan_message_id(&self, _package: &TransferPackage) -> Option<String> {
        None
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
///
/// Nothing is sent (mock partners included) until the release's
/// distribution agreement is SIGNED, which needs staff approval and the
/// artist's signature. Until then E-0 succeeds with no jobs; signing the
/// agreement queues E-0 again (`portal::sign_document`).
pub async fn enqueue_delivery_jobs(pool: &PgPool, package_id: Uuid) -> Result<(Vec<Uuid>, Uuid)> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT dp.org_id, dp.id AS package_id, pa.route_plan, pa.release_id
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
    let release_id: Uuid = row.get("release_id");
    let signed: bool = sqlx::query_scalar("SELECT execution.agreement_signed($1,$2)")
        .bind(org_id)
        .bind(release_id)
        .fetch_one(&mut *tx)
        .await?;
    if !signed {
        crate::operations::audit(
            &mut tx,
            None,
            Some(org_id),
            Some(package_id),
            "delivery.held",
            "AGREEMENT_NOT_SIGNED",
            Uuid::new_v4(),
        )
        .await?;
        tx.commit().await?;
        return Ok((Vec::new(), org_id));
    }
    let route_plan: Value = row.get("route_plan");
    // Borrow the plan items; the old `.cloned()` copied the whole array.
    let dsp_ids: Vec<Uuid> = route_plan
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            item.pointer("/scope/dsp_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .collect();
    // Registry DSPs (D-n) additionally need an APPROVED staging row. Rows are
    // approved by the system once staff finally approve the release (0051);
    // a staff HOLD keeps a DSP out.
    let approved_codes: std::collections::HashSet<String> = sqlx::query_scalar(
        "SELECT dsp_code FROM distribution.delivery_staging
         WHERE package_id=$1 AND approval='APPROVED' AND readiness<>'CONTENT_BLOCKED'",
    )
    .bind(package_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
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
        if let Some(dsp) = crate::dsp_registry::Dsp::from_uuid(d.dsp_id)
            && !approved_codes.contains(dsp.code())
        {
            continue;
        }
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
                cr.verification_package_id, r.status AS release_status, r.org_id,
                cr.revision_id = r.current_revision_id AS current_revision
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
    // A package from an older revision (resubmitted, codes re-issued) never
    // goes out, even when the new revision is READY_FOR_DELIVERY again.
    if !row.get::<bool, _>("current_revision") {
        return Err(Error::PolicyGate("EXECUTION_PACKAGE_SUPERSEDED"));
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
    delivery: (String, FileRole),
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
    // Streamed hash: constant memory for masters up to the upload cap.
    let digest = match storage.digest(key, size_bytes as u64).await {
        Ok(d) => d,
        Err(Error::PolicyGate(_)) => return Err(Error::PolicyGate("EXECUTION_FILE_DRIFT")),
        Err(_) => return Err(Error::Storage),
    };
    if digest.size != size_bytes as u64 || digest.sha256 != sha256 {
        return Err(Error::PolicyGate("EXECUTION_FILE_TAMPERED"));
    }
    let (mut delivery_name, role) = delivery;
    if delivery_name.is_empty() {
        delivery_name = key.rsplit('/').next().unwrap_or(key).to_string();
    }
    Ok(TransferFile {
        object_key: key.to_string(),
        sha256,
        size_bytes,
        delivery_name,
        role,
        content_type,
    })
}

/// Delivery name the ERN gives a file, computed from the frozen snapshot
/// once the pinned content type is known. Empty when the snapshot has no
/// UPC (mock fixtures): the object key's last segment is used instead.
fn delivery_name(
    upc: Option<&str>,
    role: FileRole,
    disc_track: (u32, u32),
    content_type: &str,
) -> String {
    match (upc, role) {
        (Some(u), FileRole::Audio) => {
            crate::ddex_ern::audio_file_name(u, disc_track.0, disc_track.1, content_type)
        }
        (Some(u), FileRole::Artwork) => crate::ddex_ern::image_file_name(u, content_type),
        (None, _) => String::new(),
    }
}

fn has_virtual_identifier(snapshot: &Value) -> bool {
    use crate::identifiers::{IdentifierKind, is_virtual};
    let upc = snapshot
        .get("upc")
        .and_then(Value::as_str)
        .is_some_and(|u| is_virtual(IdentifierKind::Upc, u));
    let isrc = snapshot
        .get("tracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| t.get("isrc").and_then(Value::as_str))
        .any(|i| is_virtual(IdentifierKind::Isrc, i));
    upc || isrc
}

async fn materialize(
    pool: &PgPool,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    storage: &Arc<dyn ObjectStore>,
    job: &DeliveryJob,
) -> Result<TransferPackage> {
    let row = sqlx::query(
        "SELECT dp.package_hash, dp.body, cr.id AS canonical_id, cr.org_id, cr.release_id, cr.body AS snapshot,
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
    // The frozen body must hash to the stored package hash (streamed into
    // the hasher: no serialized copy of the package is built).
    if crate::domain::sha256_json(&body) != package_hash {
        return Err(Error::PolicyGate("EXECUTION_PACKAGE_TAMPERED"));
    }
    let preflight: Value = row.get("preflight_report");
    for check in ["xml", "metadata", "files", "rights"] {
        if preflight.get(check).and_then(Value::as_str) != Some("Pass") {
            return Err(Error::PolicyGate("EXECUTION_PREFLIGHT_NOT_PASS"));
        }
    }
    // Rebuild the transfer manifest from the canonical release (fetched in
    // the same round trip): every file must still exist in storage with the
    // pinned size/content type.
    let snapshot: Value = row.get("snapshot");
    let mut files = Vec::new();
    // Every file is verified against its pinned catalog.assets record and
    // then against storage bytes: existence, size, content type and SHA-256
    // must all match the pin. This mirrors the F4 preflight content check;
    // E-2 re-verifies because bytes may have changed between preparation
    // and send. Anything missing or mismatched fails closed.
    let org_id: Uuid = row.get("org_id");
    let upc = snapshot
        .get("upc")
        .and_then(Value::as_str)
        .filter(|u| !u.is_empty())
        .map(str::to_owned);
    let content_type_of = |f: &TransferFile| f.content_type.clone();
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
            let n = |k: &str| t.get(k).and_then(Value::as_u64).unwrap_or(1) as u32;
            let mut f = verify_file(
                tx,
                storage,
                org_id,
                asset_id,
                key,
                (String::new(), FileRole::Audio),
            )
            .await?;
            let name = delivery_name(
                upc.as_deref(),
                FileRole::Audio,
                (n("disc_number"), n("track_number")),
                &content_type_of(&f),
            );
            if !name.is_empty() {
                f.delivery_name = name;
            }
            files.push(f);
        }
    }
    if let Some(art) = snapshot.get("artwork").filter(|a| !a.is_null()) {
        let key = art
            .get("object_key")
            .and_then(Value::as_str)
            .ok_or(Error::PolicyGate("EXECUTION_FILE_REF_MISSING"))?;
        let asset_id: Uuid = art
            .get("asset_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::PolicyGate("EXECUTION_FILE_REF_MISSING"))?;
        let mut f = verify_file(
            tx,
            storage,
            org_id,
            asset_id,
            key,
            (String::new(), FileRole::Artwork),
        )
        .await?;
        let name = delivery_name(
            upc.as_deref(),
            FileRole::Artwork,
            (0, 0),
            &content_type_of(&f),
        );
        if !name.is_empty() {
            f.delivery_name = name;
        }
        files.push(f);
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
    // Virtual (test) UPC/ISRC codes never reach a real partner. Checked on the
    // frozen snapshot itself, so it fails closed without ledger visibility.
    if (transport != "mock" || activation_kind == "CONTRACTED") && has_virtual_identifier(&snapshot)
    {
        return Err(Error::PolicyGate("EXECUTION_VIRTUAL_IDENTIFIER"));
    }
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
    // Partner-spec DSPs (the Korean services) take the partner feed, not
    // DDEX: they get no ddex_messages row and send no ERN document.
    let partner_feed = transport == "partner"
        || dsp_id
            .and_then(crate::dsp_registry::Dsp::from_uuid)
            .is_some_and(|d| d.spec().format == crate::dsp_registry::DeliveryFormat::PartnerSpec);
    let (ern_xml, ern_sha256): (String, String) = match ddex {
        Some((xml, sha)) => (xml, sha),
        None if partner_feed && (transport != "mock" || activation_kind == "CONTRACTED") => {
            (String::new(), crate::domain::sha256_hex(b""))
        }
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
    if crate::domain::sha256_hex(&ern_xml) != ern_sha256 {
        return Err(Error::PolicyGate("EXECUTION_ERN_TAMPERED"));
    }
    let ern_xml_bytes = ern_xml.into_bytes();
    // Partner-specific feeds need the full release metadata. Rebuilt from
    // the same frozen canonical snapshot the ERN came from; best effort
    // (legacy fixtures without complete metadata simply have none, and the
    // adapters that need it fail closed).
    let canonical_id: Uuid = row.get("canonical_id");
    let prepared = match serde_json::from_value::<crate::distribution::CanonicalRelease>(snapshot) {
        Ok(c) => crate::preparation_model::PreparedRelease::from_canonical(pool, canonical_id, &c)
            .await
            .ok()
            .map(Arc::new),
        Err(_) => None,
    };
    let (genre, label) = release_extras(tx, &prepared).await;
    Ok(TransferPackage {
        package_id: job.package_id,
        package_hash,
        org_id: row.get("org_id"),
        release_id: row.get("release_id"),
        ern_xml: ern_xml_bytes,
        files,
        upc,
        prepared,
        genre,
        label,
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
    let package = match materialize(pool, &mut tx, storage, job).await {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("{e:?}");
            tx.rollback().await?;
            fail_job(pool, job, "FAILED", &msg, None, None).await?;
            return Err(e);
        }
    };
    adapter.require(adapter.capabilities().send_or_publish, "send_or_publish")?;
    // Adapter-side validation (partner rules, file names, metadata the feed
    // needs). A failure here is permanent for this package: record it on the
    // job instead of leaving the job LEASED for the reconciler to requeue
    // until its attempts run out.
    let checked = match adapter.validate_package(&package).await {
        Ok(()) => adapter.prepare_transfer(&package).await.map(|_| ()),
        Err(e) => Err(e),
    };
    if let Err(e) = checked {
        let msg = format!("{e:?}");
        tx.rollback().await?;
        fail_job(
            pool,
            job,
            "FAILED",
            &format!("ADAPTER_VALIDATION:{msg}"),
            Some("PARTNER_REJECTED"),
            Some(json!({"stage":"E-2","adapter_validation":msg})),
        )
        .await?;
        return Err(e);
    }

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
    let request_sha256 = crate::domain::sha256_hex(&package.ern_xml);
    // A partner id the adapter picks itself (DDEX batch folder) is part of
    // the attempt record before the call: after a crash or lost response
    // the inquiry asks about exactly this submission.
    let planned_message_id = adapter.plan_message_id(&package);
    let inserted = sqlx::query_scalar::<_, i32>(
        "INSERT INTO execution.delivery_attempts(id,org_id,job_id,attempt_no,idempotency_key,request_sha256,outcome,partner_message_id)
         VALUES($1,$2,$3,$4,$5,$6,'IN_FLIGHT',$7) ON CONFLICT DO NOTHING RETURNING 1",
    )
    .bind(attempt_id)
    .bind(job.org_id)
    .bind(job.id)
    .bind(attempt_no)
    .bind(&idempotency_key)
    .bind(&request_sha256)
    .bind(planned_message_id.as_deref())
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
        planned_message_id,
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
        Ok(SendOutcome::Unavailable { detail }) => {
            // Definitely not received: close this attempt (REJECTED with a
            // retryable marker; the next attempt gets a new key) and hand the
            // delivery job back for a retry with backoff.
            sqlx::query(
                "UPDATE execution.delivery_attempts SET outcome='REJECTED', response=$2 WHERE id=$1",
            )
            .bind(attempt_id)
            .bind(json!({"code": "PARTNER_UNAVAILABLE", "retryable": true, "message": detail}))
            .execute(&mut *tx)
            .await?;
            let n = sqlx::query(
                "UPDATE execution.delivery_jobs SET status='QUEUED', last_error='PARTNER_UNAVAILABLE',
                 locked_by=NULL, lock_token=NULL, lease_until=NULL, updated_at=now()
                 WHERE id=$1 AND lock_token=$2",
            )
            .bind(job.id)
            .bind(job.token)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if n != 1 {
                return Err(Error::Conflict);
            }
            tx.commit().await?;
            Ok("RETRY".to_string())
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

/// Partner event ingestion (webhook body or ACK file). Deduplicated on
/// (partner, event id) in `execution.ack_events`: a redelivered event is
/// acknowledged without re-applying state. The caller supplies the org
/// (`org_for_event` resolves it for webhooks).
///
/// Every event is correlated to ONE delivery: by the partner message id
/// (our attempt), else by the partner's release id. A LIVE or takedown
/// event that matches nothing is UNMATCHED — it never flips other
/// releases of the same partner (it used to mark every INGESTING binding
/// of the partner LIVE).
pub async fn ingest_ack(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    payload: &[u8],
) -> Result<String> {
    adapter.require(adapter.capabilities().parse_ack, "parse_ack")?;
    let event = adapter.parse_ack(payload).await?;
    apply_ack_event(pool, org, adapter.partner_id(), &event).await
}

/// Ids an event carries for correlation: (event id, outcome, message id,
/// release id, extra response fields).
fn ack_parts(event: &AckEvent) -> (&str, &'static str, Option<&str>, Option<&str>, Value) {
    match event {
        AckEvent::Accepted {
            event_id,
            partner_message_id,
        } => (
            event_id,
            "ACCEPTED",
            Some(partner_message_id.as_str()),
            None,
            json!({}),
        ),
        AckEvent::Rejected {
            event_id,
            partner_message_id,
            code,
        } => (
            event_id,
            "REJECTED",
            Some(partner_message_id.as_str()),
            None,
            json!({"code": code}),
        ),
        AckEvent::Live {
            event_id,
            partner_release_id,
            partner_message_id,
        } => (
            event_id,
            "LIVE",
            partner_message_id.as_deref(),
            Some(partner_release_id.as_str()),
            json!({}),
        ),
        AckEvent::TakedownConfirmed {
            event_id,
            partner_release_id,
            partner_message_id,
        } => (
            event_id,
            "TAKEN_DOWN",
            partner_message_id.as_deref(),
            partner_release_id.as_deref(),
            json!({}),
        ),
    }
}

/// Which org a partner event belongs to (webhooks carry partner ids only).
pub async fn org_for_event(
    pool: &PgPool,
    partner_id: &str,
    event: &AckEvent,
) -> Result<Option<Uuid>> {
    let (_, _, pmid, prid, _) = ack_parts(event);
    Ok(
        sqlx::query_scalar("SELECT execution.partner_event_org($1,$2,$3)")
            .bind(partner_id)
            .bind(pmid)
            .bind(prid)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn apply_ack_event(
    pool: &PgPool,
    org: Uuid,
    partner_id: &str,
    event: &AckEvent,
) -> Result<String> {
    let (event_id, outcome, pmid, prid, extra) = ack_parts(event);
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let seen: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.ack_events WHERE partner_id=$1 AND event_id=$2)",
    )
    .bind(partner_id)
    .bind(event_id)
    .fetch_one(&mut *tx)
    .await?;
    if seen {
        tx.rollback().await?;
        return Ok("DUPLICATE_IGNORED".to_string());
    }
    // Correlate to exactly one delivery job of this partner.
    let by_message: Option<(Uuid, Uuid)> = match pmid {
        Some(m) => {
            sqlx::query_as(
                "SELECT j.id, j.package_id FROM execution.delivery_attempts a
                 JOIN execution.delivery_jobs j ON j.id=a.job_id
                 WHERE a.partner_message_id=$1 AND j.partner_id=$2
                 ORDER BY a.attempt_no DESC LIMIT 1",
            )
            .bind(m)
            .bind(partner_id)
            .fetch_optional(&mut *tx)
            .await?
        }
        None => None,
    };
    let job: Option<(Uuid, Uuid)> = match (by_message, prid) {
        (Some(j), _) => Some(j),
        (None, Some(r)) => {
            sqlx::query_as(
                "SELECT j.id, j.package_id FROM execution.live_bindings b
                 JOIN execution.delivery_jobs j ON j.package_id=b.package_id AND j.partner_id=b.partner_id
                 WHERE b.partner_id=$1 AND b.partner_release_id=$2
                 ORDER BY b.updated_at DESC LIMIT 1",
            )
            .bind(partner_id)
            .bind(r)
            .fetch_optional(&mut *tx)
            .await?
        }
        (None, None) if outcome == "TAKEN_DOWN" => {
            // Legacy partners confirm takedowns without ids: only an
            // unambiguous single pending takedown may be confirmed.
            let pending: Vec<(Uuid, Uuid)> = sqlx::query_as(
                "SELECT j.id, j.package_id FROM execution.live_bindings b
                 JOIN execution.delivery_jobs j ON j.package_id=b.package_id AND j.partner_id=b.partner_id
                 WHERE b.partner_id=$1 AND b.live_status='TAKEDOWN_REQUESTED' LIMIT 2",
            )
            .bind(partner_id)
            .fetch_all(&mut *tx)
            .await?;
            if pending.len() == 1 {
                pending.into_iter().next()
            } else {
                None
            }
        }
        (None, None) => None,
    };
    let Some((job_id, package_id)) = job else {
        tx.rollback().await?;
        return Ok("UNMATCHED".to_string());
    };
    let marker = json!({"ack_event_id": event_id, "ack_outcome": outcome});
    match outcome {
        "ACCEPTED" | "REJECTED" => {
            sqlx::query(
                "UPDATE execution.delivery_attempts SET response = response || $3 || $4
                 WHERE job_id=$1 AND partner_message_id=$2",
            )
            .bind(job_id)
            .bind(pmid)
            .bind(&marker)
            .bind(&extra)
            .execute(&mut *tx)
            .await?;
            if outcome == "ACCEPTED" {
                sqlx::query(
                    "UPDATE execution.delivery_jobs SET status='DELIVERED', updated_at=now()
                     WHERE id=$1 AND status IN ('SENDING','AWAITING_RECONCILIATION')",
                )
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE execution.delivery_jobs SET status='FAILED', last_error='PARTNER_WEBHOOK_REJECTED', updated_at=now()
                     WHERE id=$1",
                )
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
            }
        }
        "LIVE" => {
            // A live signal is also proof of receipt for the reconciler.
            sqlx::query(
                "UPDATE execution.delivery_attempts SET response = response || $2
                 WHERE id = (SELECT id FROM execution.delivery_attempts WHERE job_id=$1
                             AND partner_message_id IS NOT NULL ORDER BY attempt_no DESC LIMIT 1)
                   AND NOT response ? 'ack_event_id'",
            )
            .bind(job_id)
            .bind(&marker)
            .execute(&mut *tx)
            .await?;
            let job = DeliveryJob {
                id: job_id,
                token: Uuid::nil(),
                org_id: org,
                package_id,
                partner_id: partner_id.to_string(),
                attempts: 0,
            };
            upsert_live_binding(&mut tx, &job, "LIVE", prid).await?;
        }
        _ => {
            sqlx::query(
                "UPDATE execution.live_bindings SET live_status='TAKEN_DOWN', last_checked_at=now(), updated_at=now()
                 WHERE package_id=$1 AND partner_id=$2",
            )
            .bind(package_id)
            .bind(partner_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    let inserted = sqlx::query_scalar::<_, i32>(
        "INSERT INTO execution.ack_events(partner_id,event_id,org_id,outcome,job_id,applied)
         VALUES($1,$2,$3,$4,$5,true) ON CONFLICT DO NOTHING RETURNING 1",
    )
    .bind(partner_id)
    .bind(event_id)
    .bind(org)
    .bind(outcome)
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await?;
    if inserted.is_none() {
        // A concurrent delivery of the same event won the race.
        tx.rollback().await?;
        return Ok("DUPLICATE_IGNORED".to_string());
    }
    tx.commit().await?;
    Ok("APPLIED".to_string())
}

/// E-4: poll the partner for ingest/live state and refresh live bindings.
pub async fn poll_live(
    pool: &PgPool,
    org: Uuid,
    adapter: &dyn DspAdapter,
    package_id: Uuid,
    partner_id: &str,
) -> Result<String> {
    let caps = adapter.capabilities();
    if !caps.get_release_status && !caps.inquire_submission {
        return Err(Error::Gated);
    }
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

    // Release status needs the partner's release id and the status
    // capability; until then (or for partners that only answer submission
    // inquiries, e.g. DDEX ACK files) the submission itself is asked about.
    let outcome = match &partner_release_id {
        Some(prid) if caps.get_release_status => adapter.get_release_status(prid).await?,
        _ if !caps.inquire_submission => return Ok("NO_STATUS_SOURCE".to_string()),
        _ => {
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
    // A definite partner answer from polling is receipt evidence, like an
    // ACK webhook: file-drop partners never call back, so without this the
    // reconciler opened MISSING_ACK for every polled delivery.
    let polled = match &outcome {
        InquiryOutcome::Accepted { .. } | InquiryOutcome::Live { .. } => Some("ACCEPTED"),
        InquiryOutcome::Rejected { .. } => Some("REJECTED"),
        _ => None,
    };
    if let Some(ack) = polled {
        sqlx::query(
            "UPDATE execution.delivery_attempts SET response = response || jsonb_build_object('ack_event_id', 'poll:' || id::text, 'ack_outcome', $3::text)
             WHERE id = (SELECT a.id FROM execution.delivery_attempts a JOIN execution.delivery_jobs j ON j.id=a.job_id
                         WHERE j.package_id=$1 AND j.partner_id=$2 AND a.partner_message_id IS NOT NULL
                         ORDER BY a.attempt_no DESC LIMIT 1)
               AND NOT response ? 'ack_event_id'",
        )
        .bind(package_id)
        .bind(partner_id)
        .bind(ack)
        .execute(&mut *tx)
        .await?;
    }
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
            // A partner rejection found by polling needs a human, like one
            // returned by the send itself.
            let job = DeliveryJobRef {
                id: binding.get("job_id"),
                org_id: binding.get("org_id"),
            };
            open_case_for(&mut tx, &job, "PARTNER_REJECTED").await?;
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
    // Only orgs with open delivery work (migration 0055): the sweep used to
    // open a transaction for every organization on the platform. Each org
    // is still authorized before its RLS-protected rows are touched.
    let orgs: Vec<Uuid> = sqlx::query_scalar("SELECT execution.orgs_with_open_deliveries()")
        .fetch_all(pool)
        .await?;
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
         AND j.updated_at < now() - make_interval(secs=>$1)
         -- Cases open on the first sweep past the deadline; there is no
         -- need to rescan years of delivered history every 15 minutes.
         AND j.updated_at > now() - interval '30 days'",
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
    // Repairs are not reconciliation cases; the return value keeps counting
    // opened cases only.
    repair_stalled_sends(&mut tx, org).await?;
    tx.commit().await?;
    Ok(opened)
}

/// Sandbox round 2: a worker killed while holding a delivery lease left the
/// delivery job LEASED with its send job dead-lettered, so the release
/// showed READY_FOR_DELIVERY but was never sent, and nothing repaired it.
///
/// A delivery job that is QUEUED, or LEASED with an expired lease, has not
/// started a wire send (SENDING is the state that may have reached the
/// partner; it is never touched here and stays with the SENT_UNKNOWN
/// inquiry path). If no send job is alive for it, the lease is cleared and
/// a fresh send job is enqueued. Exactly-once is preserved: only the holder
/// of the delivery-job lease can send, and `lease_delivery_job` grants it to
/// one caller at a time.
pub async fn repair_stalled_sends(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org: Uuid,
) -> Result<usize> {
    let stalled: Vec<(Uuid, i32)> = sqlx::query_as(
        "SELECT j.id, j.attempts FROM execution.delivery_jobs j
          WHERE j.org_id=$1 AND j.attempts < j.max_attempts
            AND (j.status='QUEUED' OR (j.status='LEASED' AND j.lease_until < now()))
            AND NOT EXISTS (
              SELECT 1 FROM operations.jobs o
               WHERE o.kind='delivery.send' AND o.status IN ('QUEUED','RUNNING')
                 AND o.payload->>'delivery_job_id' = j.id::text)
          FOR UPDATE OF j SKIP LOCKED",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    for (id, attempts) in &stalled {
        sqlx::query(
            "UPDATE execution.delivery_jobs SET status='QUEUED', locked_by=NULL, lock_token=NULL,
             lease_until=NULL, last_error='REQUEUED_BY_RECONCILE', updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .execute(&mut **tx)
        .await?;
        crate::operations::enqueue(
            tx,
            "delivery",
            "delivery.send",
            &serde_json::json!({"delivery_job_id": id, "org_id": org}),
            &format!("delivery.send:{id}:repair:{attempts}"),
            None,
        )
        .await?;
        tracing::warn!(delivery_job_id=%id, "reconcile requeued a stalled delivery send");
    }
    Ok(stalled.len())
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

/// Follow-up DDEX message (metadata update or takedown) for a package that
/// was already delivered to `partner_id`. Built from the same frozen
/// canonical snapshot, addressed to the same sender/recipient pair, in the
/// original message's thread so the partner correlates it. Partners without
/// a stored DDEX message (the mock, partner-spec feeds) get an empty
/// document and describe the change in their own format.
async fn followup_package(
    pool: &PgPool,
    org: Uuid,
    package_id: Uuid,
    partner_id: &str,
    sub_type: crate::ddex_ern::MessageSubType,
) -> Result<TransferPackage> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let row = sqlx::query(
        "SELECT dp.package_hash, cr.id AS canonical_id, cr.body AS snapshot, cr.release_id,
                m.ern_xml, m.sender_name, m.sender_dpid, m.recipient_name, m.recipient_dpid
         FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         LEFT JOIN execution.adapter_profiles p ON p.partner_id=$2
         LEFT JOIN distribution.ddex_messages m ON m.package_id=dp.id AND m.dsp_id=p.dsp_id
         WHERE dp.id=$1",
    )
    .bind(package_id)
    .bind(partner_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    tx.commit().await?;
    let snapshot: Value = row.get("snapshot");
    let upc = snapshot
        .get("upc")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let canonical: Option<crate::distribution::CanonicalRelease> =
        serde_json::from_value(snapshot).ok();
    let prepared = match &canonical {
        Some(c) => crate::preparation_model::PreparedRelease::from_canonical(
            pool,
            row.get("canonical_id"),
            c,
        )
        .await
        .ok()
        .map(Arc::new),
        None => None,
    };
    let mut ern_xml = Vec::new();
    if let (Some(original), Some(p)) = (row.get::<Option<String>, _>("ern_xml"), &prepared) {
        let first = |tag: &str| {
            crate::transport::xml_values(&original, tag)
                .into_iter()
                .next()
                .unwrap_or_default()
        };
        let original_id = first("MessageId");
        let now = chrono::Utc::now();
        let suffix = match sub_type {
            crate::ddex_ern::MessageSubType::Takedown => "TD",
            _ => "UPD",
        };
        let config = crate::ddex_ern::DdexErnConfig {
            deal: crate::dsp_registry::Dsp::from_code(partner_id)
                .map(|d| d.spec().deal)
                .unwrap_or(&crate::ddex_ern::DEAL_SUBSCRIPTION),
            message_id: format!("{original_id}-{suffix}{}", now.format("%Y%m%d%H%M%S")),
            message_thread_id: Some(original_id),
            message_sub_type: sub_type,
            created_at: now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            sender_name: row.get("sender_name"),
            sender_party_id: Some(row.get("sender_dpid")),
            sent_on_behalf_of: None,
            recipient_name: row.get("recipient_name"),
            recipient_party_id: Some(row.get("recipient_dpid")),
            deal_start_date: first("StartDate"),
            takedown_date: (sub_type == crate::ddex_ern::MessageSubType::Takedown)
                .then(|| now.format("%Y-%m-%d").to_string()),
        };
        let xml = crate::ddex_ern::generate_ddex_ern_382(p, &config)?;
        ern_xml = xml.into_bytes();
    }
    let mut c = pool.acquire().await?;
    let (genre, label) = release_extras(&mut c, &prepared).await;
    Ok(TransferPackage {
        package_id,
        package_hash: row.get("package_hash"),
        org_id: org,
        release_id: row.get("release_id"),
        ern_xml,
        files: Vec::new(),
        upc,
        prepared,
        genre,
        label,
    })
}

/// Genre and label of the frozen application (same draft the prepared
/// release was built from). Best effort: None when unavailable.
async fn release_extras(
    c: &mut PgConnection,
    prepared: &Option<Arc<crate::preparation_model::PreparedRelease>>,
) -> (Option<String>, Option<String>) {
    let Some(p) = prepared else {
        return (None, None);
    };
    let draft: Option<Value> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(ar.body -> 'release' -> 'draft', 'null'::jsonb), r.draft)
         FROM catalog.application_revisions ar
         JOIN catalog.releases r ON r.org_id=ar.org_id AND r.id=ar.release_id
         WHERE ar.org_id=$1 AND ar.id=$2",
    )
    .bind(p.org_id)
    .bind(p.revision_id)
    .fetch_optional(c)
    .await
    .ok()
    .flatten();
    let Some(d) = draft else {
        return (None, None);
    };
    let text = |k: &str| {
        d.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let genre = match text("genre").as_deref() {
        Some("__other__") => text("genreCustom"),
        _ => text("genre"),
    };
    let label = text("label").or_else(|| text("label_name"));
    (genre, label)
}

/// The partner's id for the accepted submission of a job (API partners
/// address updates and takedowns to it).
async fn accepted_message_id(pool: &PgPool, org: Uuid, job_id: Uuid) -> Result<Option<String>> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let id: Option<String> = sqlx::query_scalar(
        "SELECT partner_message_id FROM execution.delivery_attempts
         WHERE job_id=$1 AND outcome='ACCEPTED' AND partner_message_id IS NOT NULL
         ORDER BY attempt_no DESC LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    tx.commit().await?;
    Ok(id)
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
    let _: Uuid = row.get("org_id");
    let package = followup_package(
        pool,
        org,
        package_id,
        partner_id,
        crate::ddex_ern::MessageSubType::Update,
    )
    .await?;
    let ctx = SendContext {
        job_id,
        attempt_id: Uuid::new_v4(),
        attempt_no: 0,
        idempotency_key: format!("update:{job_id}:{}", Uuid::new_v4().simple()),
        planned_message_id: accepted_message_id(pool, org, job_id).await?,
        package,
    };
    match adapter.update_release(&ctx, changes).await? {
        SendOutcome::Accepted { .. } => Ok("UPDATE_ACCEPTED".to_string()),
        SendOutcome::Rejected { .. } => Err(Error::PolicyGate("UPDATE_REJECTED")),
        SendOutcome::Timeout | SendOutcome::Unknown { .. } => {
            Err(Error::PolicyGate("UPDATE_UNKNOWN"))
        }
        SendOutcome::Unavailable { .. } => Err(Error::Storage),
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
    let package = followup_package(
        pool,
        org,
        package_id,
        partner_id,
        crate::ddex_ern::MessageSubType::Takedown,
    )
    .await?;
    let ctx = SendContext {
        job_id,
        attempt_id: Uuid::new_v4(),
        attempt_no: 0,
        idempotency_key: format!("takedown:{job_id}"),
        planned_message_id: accepted_message_id(pool, org, job_id).await?,
        package,
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
        SendOutcome::Unavailable { .. } => {
            tx.rollback().await?;
            Err(Error::Storage)
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

/// Hand a leased delivery job back (QUEUED, lease cleared, attempt not
/// counted as a send) when the worker cannot even start: e.g. no adapter
/// is configured for the partner yet. The reconciler requeues it later.
pub async fn release_delivery_lease(pool: &PgPool, job: &DeliveryJob, reason: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, job.org_id).await?;
    sqlx::query(
        "UPDATE execution.delivery_jobs SET status='QUEUED', locked_by=NULL, lock_token=NULL,
         lease_until=NULL, attempts=GREATEST(attempts-1,0), last_error=$3, updated_at=now()
         WHERE id=$1 AND lock_token=$2",
    )
    .bind(job.id)
    .bind(job.token)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Staff-recorded LIVE evidence (a partner that never reports live status,
/// or a release confirmed in the partner's catalogue by hand). Only a
/// DELIVERED job can be marked; the binding keeps the partner release id
/// when one is given.
pub async fn record_manual_live(
    pool: &PgPool,
    package_id: Uuid,
    partner_id: &str,
    partner_release_id: Option<&str>,
    staff_user: Option<Uuid>,
) -> Result<String> {
    let org: Uuid =
        sqlx::query_scalar("SELECT org_id FROM distribution.distribution_packages WHERE id=$1")
            .bind(package_id)
            .fetch_optional(pool)
            .await?
            .ok_or(Error::NotFound)?;
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let job: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, status FROM execution.delivery_jobs WHERE package_id=$1 AND partner_id=$2",
    )
    .bind(package_id)
    .bind(partner_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((job_id, status)) = job else {
        return Err(Error::NotFound);
    };
    if status != "DELIVERED" {
        return Err(Error::PolicyGate("DELIVERY_NOT_DELIVERED"));
    }
    let job = DeliveryJob {
        id: job_id,
        token: Uuid::nil(),
        org_id: org,
        package_id,
        partner_id: partner_id.to_string(),
        attempts: 0,
    };
    upsert_live_binding(&mut tx, &job, "LIVE", partner_release_id).await?;
    crate::operations::audit(
        &mut tx,
        staff_user,
        Some(org),
        Some(job_id),
        "delivery.live_recorded",
        "STAFF_EVIDENCE",
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok("LIVE".to_string())
}

/// Why a delivery job could not be leased (sandbox round 2: a worker killed
/// mid-send left the job LEASED; the dispatcher then returned without
/// finishing its own job, burning one attempt per job lease until the send
/// was dead-lettered while the delivery job stayed LEASED forever).
#[derive(Debug, Clone, PartialEq)]
pub enum LeaseBlocked {
    /// Another (possibly crashed) holder's lease runs for this many seconds.
    HeldFor(i64),
    /// attempts reached max_attempts; the job is now DEAD_LETTER.
    Exhausted,
    /// Terminal or missing: nothing left to send.
    Gone,
}

/// Explain a failed `lease_delivery_job`, dead-lettering the delivery job
/// itself when its attempts are exhausted so the state is never a silent
/// LEASED zombie.
pub async fn delivery_lease_blocked(
    pool: &PgPool,
    org: Uuid,
    job_id: Uuid,
) -> Result<LeaseBlocked> {
    let mut tx = pool.begin().await?;
    authorize_org(&mut tx, org).await?;
    let row = sqlx::query(
        "SELECT status, attempts, max_attempts,
                GREATEST(0, CEIL(EXTRACT(EPOCH FROM (lease_until - now()))))::bigint AS remaining
         FROM execution.delivery_jobs WHERE id=$1 FOR UPDATE",
    )
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(LeaseBlocked::Gone);
    };
    let status: String = row.get("status");
    let out = if !matches!(status.as_str(), "QUEUED" | "LEASED" | "SENDING") {
        LeaseBlocked::Gone
    } else if row.get::<i32, _>("attempts") >= row.get::<i32, _>("max_attempts") {
        sqlx::query(
            "UPDATE execution.delivery_jobs SET status='DEAD_LETTER', lock_token=NULL,
             lease_until=NULL, last_error='DELIVERY_ATTEMPTS_EXHAUSTED', updated_at=now()
             WHERE id=$1",
        )
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        LeaseBlocked::Exhausted
    } else {
        LeaseBlocked::HeldFor(row.get::<Option<i64>, _>("remaining").unwrap_or(0).max(1))
    };
    tx.commit().await?;
    Ok(out)
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
