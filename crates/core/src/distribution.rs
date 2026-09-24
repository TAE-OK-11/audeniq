//! F4 Stage 3 prep (BLUEPRINT §6 3-A/3-D/3-F), Muse portion.
//!
//! `build_canonical` pins the Stage 2 verification package — its hash, the
//! approved DSP set, the policy rule version and the rights epoch — into one
//! byte-immutable canonical snapshot (`distribution.canonical_releases`).
//! `freeze_package` then content-addresses that snapshot into a frozen
//! distribution package (`distribution.distribution_packages`, status
//! `PREPARED`).
//!
//! What this module deliberately does NOT do (Astra's half, migrations
//! 0011+): identifier allocation/reservation, route planning, ERN generation,
//! scheduling and preflight. Those stages consume `distribution_packages`
//! rows and extend `status`; they never mutate the canonical snapshot.

use crate::{
    error::{Error, Result},
    operations,
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
        "SELECT t.id, t.title, t.disc_number, t.track_number, t.artist_id, t.asset_id, t.isrc, a.name AS artist_name, s.sha256 AS asset_sha256, s.object_key AS asset_object_key
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
            disc_number: t.get("disc_number"),
            track_number: t.get("track_number"),
            artist_id: t.get("artist_id"),
            artist_name: t.get("artist_name"),
            asset_id: t.get("asset_id"),
            asset_sha256: t.get("asset_sha256"),
            asset_object_key: t.get("asset_object_key"),
            isrc: t.get("isrc"),
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

/// Freshness guard: if the rights epoch moved since Stage 2 pinned it, the
/// release goes back to Stage 2 for re-verification (BLUEPRINT §6.1
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

/// 3-A/3-D/3-F durable entry point. Replaces the F3 `park()` for
/// `prepare_release`. Returns `None` when the job lost its lease: the caller
/// must neither succeed nor fail the job; the sweeper will reclaim it.
pub async fn run_prepare_release(
    pool: &PgPool,
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
            "SELECT cr.release_id, cr.verification_package_id, dp.canonical_release_id, dp.package_hash, r.status
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
            return Ok(Some(PrepareSummary {
                revision_id,
                verification_package_id: row.get("verification_package_id"),
                canonical_release_id: row.get("canonical_release_id"),
                package_id: pkg_id,
                package_hash: row.get("package_hash"),
                release_status: status,
                returned_to_s2: false,
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
    operations::audit(
        &mut tx2,
        None,
        Some(org),
        Some(revision_id),
        "stage3.prepared",
        "PREPARE_RELEASE_READY",
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
    }))
}
