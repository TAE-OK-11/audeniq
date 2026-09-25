//! F4 Stage 3 prep (BLUEPRINT §6 3-A/3-D/3-F) plus the merged preparation
//! pipeline: after the canonical snapshot is frozen, the worker builds
//! Astra's `PreparedRelease` supplements from the pinned snapshot, generates
//! the synthetic ERN, and runs the four independent preflight checks
//! (XML / metadata / files / rights) with a route plan. Anything missing
//! (UPC, artwork, ISRC, file bytes, stale pin) fails closed here — the
//! release never reaches READY_FOR_DELIVERY on an unchecked package.

use std::sync::Arc;

use crate::{
    ddex_ern, ddex_xsd,
    domain::FreshnessPin,
    ern,
    error::{Error, Result},
    identifiers::{self, ExistingAssignment, IdentifierKind},
    operations,
    preflight::{self, CurrentFacts},
    preparation_model, route_plan,
    storage::ObjectStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

/// Rule version for Stage 3 prep checks.
pub const DISTRIBUTION_RULE_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalCredit {
    pub party_id: Uuid,
    pub party_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTrack {
    pub track_id: Uuid,
    pub title: String,
    /// Version/designation ("Radio Edit"). Empty = none; emitted as
    /// DDEX SubTitle only when non-empty (ERN 3.8.2 has no VersionTitle element).
    #[serde(default)]
    pub version: String,
    pub disc_number: i32,
    pub track_number: i32,
    pub artist_id: Uuid,
    pub artist_name: String,
    pub asset_id: Option<Uuid>,
    pub asset_sha256: Option<String>,
    /// Storage object key for the audio file (ERN resource reference).
    pub asset_object_key: Option<String>,
    pub isrc: Option<String>,
    pub credits: Vec<CanonicalCredit>,
    #[serde(default)]
    pub parental_advisory: bool,
}

/// Cover artwork reference for DDEX ERN resource lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtwork {
    pub asset_id: Uuid,
    pub object_key: String,
    pub sha256: Option<String>,
    pub content_type: String,
}

/// The canonical release snapshot. The four pinned Stage 2 outputs
/// (`verification_package_hash`, `approved_dsp_ids`, `rule_version`,
/// `rights_epoch`) travel with the snapshot so any downstream stage can
/// re-verify the pin without re-reading Stage 2 state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRelease {
    /// Bumped to 2: added `upc`, `artwork`, `asset_object_key` for DDEX ERN.
    pub schema_version: u8,
    pub rule_version: String,
    pub org_id: Uuid,
    pub release_id: Uuid,
    pub revision_id: Uuid,
    pub revision_hash: String,
    pub verification_package_id: Uuid,
    pub verification_package_hash: String,
    pub rights_epoch: i64,
    pub approved_dsp_ids: Vec<Uuid>,
    pub release_title: String,
    pub release_type: String,
    /// Release-level identifier (UPC/EAN). None = not assigned yet; Stage 3
    /// never issues identifiers, only carries them.
    pub upc: Option<String>,
    pub artwork: Option<CanonicalArtwork>,
    pub tracks: Vec<CanonicalTrack>,
    /// True when any track carries parental advisory or the Stage 2
    /// verification package recorded the EXPLICIT special flag.
    #[serde(default)]
    pub explicit: bool,
}

impl CanonicalRelease {
    fn body(&self) -> Value {
        json!(self)
    }
    pub fn canonical_hash(&self) -> String {
        sha256_hex(&serde_json::to_string(&self.body()).expect("canonical serializes"))
    }
}

pub struct PrepareSummary {
    pub revision_id: Uuid,
    pub verification_package_id: Uuid,
    pub canonical_release_id: Uuid,
    pub package_id: Uuid,
    pub package_hash: String,
    pub release_status: String,
    pub returned_to_s2: bool,
    /// Real DDEX ERN 3.8.2 messages persisted for DSPs with configured DPIDs.
    pub ddex_messages: usize,
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn parse_dsp_ids(v: &Value) -> Result<Vec<Uuid>> {
    let arr = v
        .pointer("/approved_scope/dsp_ids")
        .and_then(Value::as_array)
        .ok_or(Error::PolicyGate("VERIFICATION_APPROVED_SCOPE_MISSING"))?;
    arr.iter()
        .map(|x| {
            x.as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(Error::PolicyGate("VERIFICATION_APPROVED_SCOPE_MISSING"))
        })
        .collect()
}

/// Read catalog + review state and build the canonical snapshot for a
/// Stage 2 verification package. Pure read; the caller decides when to persist.
pub async fn build_canonical(
    pool: &PgPool,
    verification_package_id: Uuid,
) -> Result<CanonicalRelease> {
    let vp = sqlx::query(
        "SELECT org_id, revision_id, body, package_hash, rights_epoch FROM distribution.verification_packages WHERE id=$1",
    )
    .bind(verification_package_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = vp.get("org_id");
    let revision_id: Uuid = vp.get("revision_id");
    let body: Value = vp.get("body");
    let package_hash: String = vp.get("package_hash");
    let rights_epoch: i64 = vp.get("rights_epoch");
    if body.get("decision").and_then(Value::as_str) != Some("PASS") {
        return Err(Error::PolicyGate("VERIFICATION_NOT_PASSED"));
    }
    let revision_hash: String = body
        .get("revision_hash")
        .and_then(Value::as_str)
        .ok_or(Error::PolicyGate("VERIFICATION_REVISION_HASH_MISSING"))?
        .into();
    let rule_version: String = body
        .get("rule_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .into();
    let approved_dsp_ids = parse_dsp_ids(&body)?;

    let rev = sqlx::query(
        "SELECT release_id FROM catalog.application_revisions WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(revision_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)?;
    let release_id: Uuid = rev.get("release_id");

    let rel =
        sqlx::query("SELECT title, release_type, upc, artwork_asset_id FROM catalog.releases WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(release_id)
            .fetch_optional(pool)
            .await?
            .ok_or(Error::NotFound)?;
    let release_title: String = rel.get("title");
    let release_type: String = rel.get("release_type");

    let tracks = sqlx::query(
        "SELECT t.id, t.title, t.version, t.disc_number, t.track_number, t.artist_id, t.asset_id, t.isrc, t.parental_advisory, a.name AS artist_name, s.sha256 AS asset_sha256, s.object_key AS asset_object_key
         FROM catalog.tracks t
         JOIN catalog.artists a ON a.org_id=t.org_id AND a.id=t.artist_id
         LEFT JOIN catalog.assets s ON s.org_id=t.org_id AND s.id=t.asset_id
         WHERE t.org_id=$1 AND t.release_id=$2
         ORDER BY t.disc_number, t.track_number, t.id",
    )
    .bind(org)
    .bind(release_id)
    .fetch_all(pool)
    .await?;
    let mut out_tracks = Vec::with_capacity(tracks.len());
    for t in &tracks {
        let track_id: Uuid = t.get("id");
        let credits = sqlx::query(
            "SELECT c.party_id, p.display_name, c.role FROM catalog.credits c
             JOIN identity.parties p ON p.org_id=c.org_id AND p.id=c.party_id
             WHERE c.org_id=$1 AND c.track_id=$2 ORDER BY c.role, p.display_name",
        )
        .bind(org)
        .bind(track_id)
        .fetch_all(pool)
        .await?;
        out_tracks.push(CanonicalTrack {
            track_id,
            title: t.get("title"),
            version: t.get("version"),
            disc_number: t.get("disc_number"),
            track_number: t.get("track_number"),
            artist_id: t.get("artist_id"),
            artist_name: t.get("artist_name"),
            asset_id: t.get("asset_id"),
            asset_sha256: t.get("asset_sha256"),
            asset_object_key: t.get("asset_object_key"),
            isrc: t.get("isrc"),
            parental_advisory: t.get("parental_advisory"),
            credits: credits
                .iter()
                .map(|c| CanonicalCredit {
                    party_id: c.get("party_id"),
                    party_name: c.get("display_name"),
                    role: c.get("role"),
                })
                .collect(),
        });
    }

    let upc: Option<String> = rel.get("upc");
    let artwork_asset_id: Option<Uuid> = rel.get("artwork_asset_id");
    let artwork = match artwork_asset_id {
        Some(aid) => {
            let a = sqlx::query(
                "SELECT object_key, sha256, content_type FROM catalog.assets WHERE org_id=$1 AND id=$2",
            )
            .bind(org)
            .bind(aid)
            .fetch_optional(pool)
            .await?
            .ok_or(Error::NotFound)?;
            Some(CanonicalArtwork {
                asset_id: aid,
                object_key: a.get("object_key"),
                sha256: a.get("sha256"),
                content_type: a.get("content_type"),
            })
        }
        None => None,
    };

    Ok(CanonicalRelease {
        schema_version: 2,
        rule_version,
        org_id: org,
        release_id,
        revision_id,
        revision_hash,
        verification_package_id,
        verification_package_hash: package_hash,
        rights_epoch,
        approved_dsp_ids,
        release_title,
        release_type,
        upc,
        artwork,
        explicit: out_tracks.iter().any(|t| t.parental_advisory)
            || body
                .get("special_flags")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(|f| f.as_str() == Some("EXPLICIT")))
                .unwrap_or(false),
        tracks: out_tracks,
    })
}

/// Persist the canonical snapshot (idempotent on verification_package_id) and
/// return its row id.
pub async fn store_canonical(tx: &mut PgConnection, canonical: &CanonicalRelease) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM distribution.canonical_releases WHERE verification_package_id=$1",
    )
    .bind(canonical.verification_package_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO distribution.canonical_releases(id, org_id, release_id, revision_id, verification_package_id, canonical_hash, body) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id)
        .bind(canonical.org_id)
        .bind(canonical.release_id)
        .bind(canonical.revision_id)
        .bind(canonical.verification_package_id)
        .bind(canonical.canonical_hash())
        .bind(canonical.body())
        .execute(&mut *tx)
        .await?;
    Ok(id)
}

/// Freeze a canonical snapshot into a content-addressed distribution package.
/// Idempotent: the same canonical snapshot always resolves to the same
/// package row. `status` starts at `PREPARED`; Astra's route/ERN stages
/// extend it.
pub async fn freeze_package(pool: &PgPool, canonical: &CanonicalRelease) -> Result<Uuid> {
    let canonical_id = {
        let mut tx = pool.begin().await?;
        let id = store_canonical(&mut tx, canonical).await?;
        tx.commit().await?;
        id
    };
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM distribution.distribution_packages WHERE canonical_release_id=$1",
    )
    .bind(canonical_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let body = json!({
        "schema_version": 1,
        "rule_version": DISTRIBUTION_RULE_VERSION,
        "canonical_release_id": canonical_id,
        "canonical_hash": canonical.canonical_hash(),
        "verification_package_hash": canonical.verification_package_hash,
        "approved_dsp_ids": canonical.approved_dsp_ids,
        // Astra's stages fill these in later; the frozen package ships with
        // explicit empty placeholders so "not yet routed" is distinguishable
        // from "routed to nothing".
        "identifier_refs": [],
        "route_id": null,
        "dsp_packages": [],
        "preflight_ref": null,
        "queued_job_refs": [],
    });
    let package_hash = sha256_hex(&serde_json::to_string(&body).expect("package serializes"));
    let id = Uuid::new_v4();
    // A concurrent freeze for the same snapshot cannot happen under one
    // lease, but tolerate the race: the UNIQUE constraint keeps one row.
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO distribution.distribution_packages(id, org_id, canonical_release_id, package_hash, body, status) VALUES($1,$2,$3,$4,$5,'PREPARED') ON CONFLICT(canonical_release_id) DO NOTHING RETURNING id",
    )
    .bind(id)
    .bind(canonical.org_id)
    .bind(canonical_id)
    .bind(&package_hash)
    .bind(&body)
    .fetch_optional(pool)
    .await?;
    match inserted {
        Some(row_id) => Ok(row_id),
        None => sqlx::query_scalar(
            "SELECT id FROM distribution.distribution_packages WHERE canonical_release_id=$1",
        )
        .bind(canonical_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into),
    }
}

/// Freshness guard: if the rights epoch moved since Stage 2 pinned it, the/// release goes back to Stage 2 for re-verification (BLUEPRINT §6.1
/// `return_to=S2`). Returns true when the caller must stop here.
async fn return_to_s2_if_epoch_moved(
    tx: &mut PgConnection,
    org: Uuid,
    release_id: Uuid,
    revision_id: Uuid,
    pinned_epoch: i64,
    request: Uuid,
) -> Result<bool> {
    let current: i64 = sqlx::query_scalar(
        "SELECT epoch FROM rights.rights_epochs WHERE org_id=$1 AND release_id=$2",
    )
    .bind(org)
    .bind(release_id)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(0);
    if current == pinned_epoch {
        return Ok(false);
    }
    // STAGE3_PREPARING -> STAGE3_CORRECTION is an allowed transition; the
    // release waits there until Stage 2 re-verifies the new epoch.
    sqlx::query("UPDATE catalog.releases SET status='STAGE3_CORRECTION', row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND status='STAGE3_PREPARING'")
        .bind(org)
        .bind(release_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO operations.jobs(id, queue, kind, payload, pinned_revision_id, idempotency_key) VALUES($1,'rights','stage2',$2,$3,$4) ON CONFLICT(idempotency_key) DO NOTHING")
        .bind(Uuid::new_v4())
        .bind(json!({"revision_id": revision_id, "reason": "RIGHTS_EPOCH_CHANGED"}))
        .bind(revision_id)
        .bind(format!("stage2-rerun:{revision_id}"))
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut *tx,
        None,
        Some(org),
        Some(revision_id),
        "stage3.return_to_s2",
        "RIGHTS_EPOCH_CHANGED",
        request,
    )
    .await?;
    Ok(true)
}

/// A ledger conflict means the identifier is already assigned to a *different*
/// release or track: retrying can never fix it, so it fails closed and the
/// dispatcher dead-letters the job for human review.
fn map_ledger_error(e: Error) -> Error {
    match e {
        Error::Conflict => Error::PolicyGate("IDENTIFIER_CONFLICT"),
        other => other,
    }
}

/// F5.5: persist one real DDEX ERN 3.8.2 `NewReleaseMessage` per DSP in the
/// frozen route plan. The internal synthetic ERN stays the preflight integrity
/// envelope; these rows are the interchange artifacts, stored in the same
/// transaction as the READY_FOR_DELIVERY flip so they are as durable as the
/// delivery handoff.
///
/// DPIDs are partner-onboarding data (F6): a DSP without a recipient DPID, or
/// an org without a sender DPID, gets no row — preparation never invents
/// party identifiers. Wire transmission of these messages is F6 work.
async fn persist_ddex_messages(
    tx: &mut PgConnection,
    org: Uuid,
    package_id: Uuid,
    prepared: &preparation_model::PreparedRelease,
    submissions: &[route_plan::SubmissionItems],
) -> Result<usize> {
    let sender: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT name, ddex_sender_dpid FROM identity.orgs WHERE id=$1")
            .bind(org)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((sender_name, Some(sender_dpid))) = sender else {
        return Ok(0);
    };
    let created_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let deal_start = prepared.release_date.format("%Y-%m-%d").to_string();
    let mut generated = 0usize;
    for s in submissions {
        let profile: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT display_name, ddex_recipient_dpid FROM execution.adapter_profiles WHERE dsp_id=$1",
    )
    .bind(s.scope.dsp_id)
    .fetch_optional(&mut *tx)
    .await?;
        let (recipient_name, recipient_dpid) = match profile {
            Some((name, Some(dpid))) => (name, dpid),
            _ => continue,
        };
        let config = ddex_ern::DdexErnConfig {
            message_id: format!("AUDENIQ-ERN-{package_id}-{}", s.scope.dsp_id),
            message_sub_type: ddex_ern::MessageSubType::Initial,
            created_at: created_at.clone(),
            sender_name: sender_name.clone(),
            sender_party_id: Some(sender_dpid.clone()),
            sent_on_behalf_of: None,
            recipient_name: recipient_name.clone(),
            recipient_party_id: Some(recipient_dpid.clone()),
            deal_start_date: deal_start.clone(),
            takedown_date: None,
        };
        let xml = ddex_ern::generate_ddex_ern_382(prepared, &config)?;
        // Contract-free F6 groundwork: every interchange message is proven
        // schema-valid before it is persisted. A message that fails XSD
        // validation never becomes a ddex_messages row (fail-closed); the
        // worker retries only when the cause is transient.
        ddex_xsd::validate_ern_382_xml(&xml)?;
        let sha = hex::encode(Sha256::digest(xml.as_bytes()));
        let res = sqlx::query(
        "INSERT INTO distribution.ddex_messages(package_id,org_id,dsp_id,sender_name,sender_dpid,recipient_name,recipient_dpid,ern_xml,ern_sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(package_id,dsp_id) DO NOTHING",
    )
    .bind(package_id)
    .bind(org)
    .bind(s.scope.dsp_id)
    .bind(&sender_name)
    .bind(&sender_dpid)
    .bind(&recipient_name)
    .bind(&recipient_dpid)
    .bind(&xml)
    .bind(&sha)
    .execute(&mut *tx)
    .await?;
        generated += usize::try_from(res.rows_affected()).unwrap_or(0);
    }
    Ok(generated)
}

/// 3-A/3-D/3-F durable entry point. Replaces the F3 `park()` for
/// `prepare_release`. Returns `None` when the job lost its lease: the caller
/// must neither succeed nor fail the job; the sweeper will reclaim it.
pub async fn run_prepare_release(
    pool: &PgPool,
    storage: &Arc<dyn ObjectStore>,
    job: &operations::Job,
) -> Result<Option<PrepareSummary>> {
    let revision_id = job
        .payload
        .get("revision_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(Error::Internal)?;
    let verification_package_id = job
        .payload
        .get("verification_package_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(Error::Internal)?;
    let request = Uuid::new_v4();

    // Idempotent completion: a previous attempt already froze the package
    // (worker crashed between commit and completion).
    if let Some(pkg_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT dp.id FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         WHERE cr.verification_package_id=$1",
    )
    .bind(verification_package_id)
    .fetch_optional(pool)
    .await?
    {
        let row = sqlx::query(
            "SELECT cr.org_id, cr.release_id, cr.verification_package_id, dp.canonical_release_id, dp.package_hash, r.status
             FROM distribution.distribution_packages dp
             JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
             JOIN catalog.releases r ON r.org_id=cr.org_id AND r.id=cr.release_id
             WHERE dp.id=$1",
        )
        .bind(pkg_id)
        .fetch_one(pool)
        .await?;
        let status: String = row.get("status");
        if status == "READY_FOR_DELIVERY" {
            // ddex_messages is FORCE RLS: without app.org_id the count
            // below would always be 0 on this pool-direct path, so the
            // retry summary would misreport ddex_messages. Authorize this
            // read's org in a short transaction.
            let org_id: Uuid = row.get("org_id");
            let mut rtx = pool.begin().await?;
            sqlx::query("SELECT set_config('app.org_id',$1,true)")
                .bind(org_id.to_string())
                .execute(&mut *rtx)
                .await?;
            let ddex_messages: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM distribution.ddex_messages WHERE package_id=$1",
            )
            .bind(pkg_id)
            .fetch_one(&mut *rtx)
            .await?;
            rtx.commit().await?;
            return Ok(Some(PrepareSummary {
                revision_id,
                verification_package_id: row.get("verification_package_id"),
                canonical_release_id: row.get("canonical_release_id"),
                package_id: pkg_id,
                package_hash: row.get("package_hash"),
                release_status: status,
                returned_to_s2: false,
                ddex_messages: usize::try_from(ddex_messages).unwrap_or(0),
            }));
        }
    }

    let mut tx = pool.begin().await?;
    // Fence every mutation on the job's live lease: an expired worker's
    // package must never commit.
    let held: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM operations.jobs WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp() FOR UPDATE",
    )
    .bind(job.id)
    .bind(job.token)
    .fetch_optional(&mut *tx)
    .await?;
    if held.is_none() {
        return Ok(None);
    }

    let vp = sqlx::query(
        "SELECT org_id, revision_id, rights_epoch FROM distribution.verification_packages WHERE id=$1",
    )
    .bind(verification_package_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = vp.get("org_id");
    let vp_revision: Uuid = vp.get("revision_id");
    let pinned_epoch: i64 = vp.get("rights_epoch");
    if vp_revision != revision_id {
        return Err(Error::PolicyGate("VERIFICATION_REVISION_MISMATCH"));
    }
    let rev = sqlx::query(
        "SELECT release_id FROM catalog.application_revisions WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(revision_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let release_id: Uuid = rev.get("release_id");

    // The worker owns Stage 3 prep now: STAGE2_PASSED -> STAGE3_PREPARING.
    // A retry that already moved forward keeps going instead of failing.
    let moved = sqlx::query("UPDATE catalog.releases SET status='STAGE3_PREPARING', row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND status='STAGE2_PASSED'")
        .bind(org)
        .bind(release_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if moved == 0 {
        let status: String =
            sqlx::query_scalar("SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2")
                .bind(org)
                .bind(release_id)
                .fetch_one(&mut *tx)
                .await?;
        if status != "STAGE3_PREPARING" {
            return Err(Error::PolicyGate("RELEASE_NOT_IN_STAGE3"));
        }
    }

    // New rights facts since Stage 2 pinned the epoch go back to Stage 2.
    if return_to_s2_if_epoch_moved(&mut tx, org, release_id, revision_id, pinned_epoch, request)
        .await?
    {
        tx.commit().await?;
        return Ok(Some(PrepareSummary {
            revision_id,
            verification_package_id,
            canonical_release_id: Uuid::nil(),
            package_id: Uuid::nil(),
            package_hash: String::new(),
            release_status: "STAGE3_CORRECTION".into(),
            returned_to_s2: true,
            ddex_messages: 0,
        }));
    }

    let canonical = build_canonical(pool, verification_package_id).await?;
    let canonical_id = store_canonical(&mut tx, &canonical).await?;
    tx.commit().await?;

    let package_id = freeze_package(pool, &canonical).await?;
    let package_hash: String = sqlx::query_scalar(
        "SELECT package_hash FROM distribution.distribution_packages WHERE id=$1",
    )
    .bind(package_id)
    .fetch_one(pool)
    .await?;

    // Stage 3 preparation: supplements + synthetic ERN + four independent
    // preflight checks (XML / metadata / files / rights) + route plan, all
    // bound to the frozen snapshot. Fail-closed: a missing UPC, artwork,
    // ISRC, file bytes or a stale pin never becomes READY_FOR_DELIVERY.
    // The canonical snapshot and frozen package stay immutable; the worker
    // persists the preparation outputs (identifier ledger entries, ERN hash,
    // preflight report, route plan) in the same commit that flips the
    // release to READY_FOR_DELIVERY.
    let prepared =
        preparation_model::PreparedRelease::from_canonical(pool, canonical_id, &canonical).await?;
    let vp = preparation_model::VerificationPackage::load(pool, verification_package_id).await?;
    let xml = ern::generate_prepared_ern(&prepared)?;
    let xml_sha = hex::encode(Sha256::digest(xml.as_bytes()));
    // Synthetic profile: no commercial route contract exists yet (F5+), so
    // the freshness pin uses a deterministic synthetic id. It still binds
    // expected vs current facts; it never authorizes a real route.
    let route_contract_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, verification_package_id.as_bytes());
    let expected = FreshnessPin {
        revision_id,
        verification_hash: vp.package_hash.clone(),
        snapshot_id: prepared.snapshot_id,
        rights_epoch: u64::try_from(canonical.rights_epoch)
            .map_err(|_| Error::PolicyGate("PREPARATION_EPOCH_INVALID"))?,
        route_contract_id,
        package_hash: xml_sha.clone(),
    };
    // Current facts are re-read from trusted state inside this boundary:
    // rights epoch from the rights table, no F4-scope hold table exists
    // (finance holds are F7), commercial contract gating is F5's job.
    let live_epoch: i64 = sqlx::query_scalar(
        "SELECT epoch FROM rights.rights_epochs WHERE org_id=$1 AND release_id=$2",
    )
    .bind(org)
    .bind(release_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)?;
    let current = CurrentFacts {
        pin: FreshnessPin {
            rights_epoch: u64::try_from(live_epoch)
                .map_err(|_| Error::PolicyGate("PREPARATION_EPOCH_INVALID"))?,
            ..expected.clone()
        },
        hold: false,
        contract_active: true,
    };
    let report =
        preflight::preflight(&prepared, &vp, &xml, &expected, &current, storage.as_ref()).await;
    if !report.passed() {
        return Err(Error::PolicyGate("PREFLIGHT_FAILED"));
    }
    let submissions = route_plan::plan_submissions(&prepared, &vp)?;

    // Freeze is done; mark the release ready for delivery prep. Actual
    // external send is F5's job, never this worker's.
    let mut tx2 = pool.begin().await?;
    let held2: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM operations.jobs WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp() FOR UPDATE",
    )
    .bind(job.id)
    .bind(job.token)
    .fetch_optional(&mut *tx2)
    .await?;
    if held2.is_none() {
        return Ok(None);
    }
    sqlx::query("UPDATE catalog.releases SET status='READY_FOR_DELIVERY', row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND status='STAGE3_PREPARING'")
        .bind(org)
        .bind(release_id)
        .execute(&mut *tx2)
        .await?;

    // The identifier ledger is RLS-protected: authorize this transaction's org.
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx2)
        .await?;
    // Record supplied identifiers in the append-only ledger, bound to this
    // revision: the release UPC plus every track ISRC. Exact-target retries
    // are idempotent; a cross-target conflict is a permanent integrity
    // failure, never something a retry can fix.
    let upc = ExistingAssignment {
        org_id: org,
        release_id,
        track_id: None,
        revision_id,
        kind: IdentifierKind::Upc,
        value: &prepared.upc,
    };
    identifiers::record_existing(&mut tx2, &upc)
        .await
        .map_err(map_ledger_error)?;
    for t in &prepared.tracks {
        let isrc = ExistingAssignment {
            org_id: org,
            release_id,
            track_id: Some(t.id),
            revision_id,
            kind: IdentifierKind::Isrc,
            value: &t.isrc,
        };
        identifiers::record_existing(&mut tx2, &isrc)
            .await
            .map_err(map_ledger_error)?;
    }

    // Persist the preparation outputs append-only, keyed by frozen package.
    // The frozen body stays the immutable canonical snapshot; ERN hash,
    // preflight report and route plan live here for audit.
    let artifact_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO distribution.preparation_artifacts(id,org_id,release_id,revision_id,canonical_release_id,package_id,ern_sha256,ern_xml,preflight_report,route_plan) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(package_id) DO NOTHING",
    )
    .bind(artifact_id)
    .bind(org)
    .bind(release_id)
    .bind(revision_id)
    .bind(canonical_id)
    .bind(package_id)
    .bind(&xml_sha)
    .bind(&xml)
    .bind(serde_json::to_value(&report).map_err(|_| Error::Internal)?)
    .bind(serde_json::to_value(&submissions).map_err(|_| Error::Internal)?)
    .execute(&mut *tx2)
    .await?;

    // F5.5: real DDEX ERN 3.8.2 interchange messages, one per DSP in the
    // frozen route plan, in the same transaction. DPIDs are partner
    // onboarding data; DSPs without one get no row (never invented).
    let ddex_generated =
        persist_ddex_messages(&mut tx2, org, package_id, &prepared, &submissions).await?;

    // Durable handoff: READY_FOR_DELIVERY and the delivery.enqueue job commit
    // atomically in this transaction. The package-scoped idempotency key
    // makes exact-target retries of this worker safe: a retry reuses the
    // existing job instead of fanning out a second delivery.
    operations::enqueue(
        &mut tx2,
        "delivery",
        "delivery.enqueue",
        &json!({"package_id": package_id}),
        &format!("delivery.enqueue:{package_id}"),
        Some(revision_id),
    )
    .await?;

    operations::audit(
        &mut tx2,
        None,
        Some(org),
        Some(revision_id),
        "stage3.prepared",
        &format!(
            "PREPARE_RELEASE_READY preflight=xml:{:?},metadata:{:?},files:{:?},rights:{:?} submissions={} ern_sha256={} artifact={artifact_id} identifiers={} ddex_messages={ddex_generated}",
            report.xml, report.metadata, report.files, report.rights,
            submissions.len(),
            xml_sha,
            prepared.tracks.len() + 1,
        ),
        request,
    )
    .await?;
    tx2.commit().await?;

    Ok(Some(PrepareSummary {
        revision_id,
        verification_package_id,
        canonical_release_id: canonical_id,
        package_id,
        package_hash,
        release_status: "READY_FOR_DELIVERY".into(),
        returned_to_s2: false,
        ddex_messages: ddex_generated,
    }))
}
