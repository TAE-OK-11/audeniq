//! Persist already-produced immutable package metadata. Not a package generator or delivery authorizer.
//! No Foundation API or worker handler exposes this owner-only repository.
use crate::{
    contracts::DistributionPackage,
    error::{Error, Result},
};
use sqlx::PgConnection;
use uuid::Uuid;

pub async fn store_package(c: &mut PgConnection, p: &DistributionPackage) -> Result<Uuid> {
    let valid_hash = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if !valid_hash(&p.package_digest)
        || !valid_hash(&p.manifest_hash)
        || p.id.is_nil()
        || p.immutable_bytes_ref.is_nil()
    {
        return Err(Error::Invalid);
    }
    let operation = serde_json::to_value(&p.operation).map_err(|_| Error::Invalid)?;
    let kind = serde_json::to_value(&p.route.kind).map_err(|_| Error::Invalid)?;
    // Validate the entire pinned route against stored immutable metadata. Never accept caller-provided routing authority.
    let route: Option<Uuid> = sqlx::query_scalar("SELECT r.id FROM distribution.route_plans r JOIN distribution.dsp_endpoints e ON e.org_id=r.org_id AND e.dsp_id=r.dsp_id AND e.id=r.endpoint_id WHERE r.org_id=$1 AND r.id=$2 AND r.dsp_id=$3 AND r.contract_id=$4 AND r.endpoint_id=$5 AND r.fee_schedule_id=$6 AND r.route_kind=$7 AND e.adapter_version=$8 AND e.profile_version=$9")
        .bind(p.org_id).bind(p.route.id).bind(p.route.dsp_id).bind(p.route.contract_id).bind(p.route.endpoint_id).bind(p.route.fee_schedule_id).bind(kind.as_str().ok_or(Error::Invalid)?).bind(&p.route.adapter_version).bind(&p.route.profile_version).fetch_optional(&mut *c).await?;
    if route.is_none() || p.route.org_id != p.org_id {
        return Err(Error::Conflict);
    }
    let inserted: Option<Uuid> = sqlx::query_scalar("INSERT INTO distribution.packages(id,org_id,snapshot_id,route_id,dsp_id,operation,adapter_version,profile_version,package_hash,manifest_hash,immutable_bytes_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING RETURNING id")
        .bind(p.id).bind(p.org_id).bind(p.snapshot_id).bind(p.route.id).bind(p.route.dsp_id).bind(operation.as_str().ok_or(Error::Invalid)?).bind(&p.route.adapter_version).bind(&p.route.profile_version).bind(&p.package_digest).bind(&p.manifest_hash).bind(p.immutable_bytes_ref).fetch_optional(&mut *c).await?;
    if let Some(id) = inserted {
        return Ok(id);
    }
    // Replays only succeed if EVERY pinned field matches; never mutate an existing package.
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM distribution.packages WHERE id=$1 AND org_id=$2 AND snapshot_id=$3 AND route_id=$4 AND dsp_id=$5 AND operation=$6 AND adapter_version=$7 AND profile_version=$8 AND package_hash=$9 AND manifest_hash=$10 AND immutable_bytes_ref=$11")
        .bind(p.id).bind(p.org_id).bind(p.snapshot_id).bind(p.route.id).bind(p.route.dsp_id).bind(operation.as_str().ok_or(Error::Invalid)?).bind(&p.route.adapter_version).bind(&p.route.profile_version).bind(&p.package_digest).bind(&p.manifest_hash).bind(p.immutable_bytes_ref).fetch_optional(c).await?;
    existing.ok_or(Error::Conflict)
}
