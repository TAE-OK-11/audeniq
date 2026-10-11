//! 발매 후 수정 (migration 0091).
//!
//! A delivered release is frozen: catalog writes only work in the correction
//! statuses, so a title or credit fixed after release never reached a DSP.
//! The owner reopens it here (`POST /api/orgs/{org}/releases/{id}/revise`):
//! READY_FOR_DELIVERY → STAGE3_CORRECTION, the signed agreement stays SIGNED
//! and UPC/ISRC stay with their targets ([`crate::identifiers`]). The
//! resubmitted revision passes Stage 1–3 again; staging never auto-approves
//! it (staff 재심사), and its delivery jobs send an ERN Update to every DSP
//! that already has the release ([`crate::execution`]).
use crate::{
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

/// True when an earlier package of this release was delivered to some DSP.
pub async fn delivered_before(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
          WHERE j.org_id=$1 AND cr.release_id=$2 AND j.status='DELIVERED')",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *c)
    .await?)
}

pub async fn open_edit(pool: &PgPool, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<String, _>("status") != "READY_FOR_DELIVERY" {
        return Err(Error::PolicyGate("RELEASE_NOT_DELIVERED"));
    }
    let signed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.documents WHERE org_id=$1 AND release_id=$2 AND kind='AGREEMENT' AND status='SIGNED')",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    if !signed || !delivered_before(&mut tx, org, release).await? {
        // Before the first delivery the application is still corrected or
        // withdrawn through the normal review flow.
        return Err(Error::PolicyGate("RELEASE_NOT_DELIVERED"));
    }
    // A send still in flight would race the edit: finish or reconcile first.
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
          WHERE j.org_id=$1 AND cr.release_id=$2
            AND j.status IN ('QUEUED','LEASED','SENDING','AWAITING_RECONCILIATION'))",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    if busy {
        return Err(Error::PolicyGate("RELEASE_DELIVERY_IN_PROGRESS"));
    }
    // A takedown / termination / transfer in progress, or distribution
    // already ended, leaves nothing to update (0098).
    crate::release_actions::ensure_distribution_open(&mut tx, release).await?;
    let held: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM finance.finance_holds WHERE org_id=$1 AND active AND scope_type='RELEASE' AND scope_value=$2)",
    )
    .bind(org)
    .bind(release.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if held {
        return Err(Error::PolicyGate("RELEASE_ON_HOLD"));
    }
    sqlx::query("SELECT set_config('audeniq.post_release_edit','1',true)")
        .execute(&mut *tx)
        .await?;
    let rv: i64 = sqlx::query_scalar(
        "UPDATE catalog.releases SET status='STAGE3_CORRECTION', row_version=row_version+1
         WHERE org_id=$1 AND id=$2 AND status='READY_FOR_DELIVERY' RETURNING row_version",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("SELECT set_config('audeniq.post_release_edit','',true)")
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.post_release_edit_opened",
        "READY_FOR_DELIVERY->STAGE3_CORRECTION",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": release, "status": "STAGE3_CORRECTION", "row_version": rv}))
}
