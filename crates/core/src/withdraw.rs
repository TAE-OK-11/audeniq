//! Cancelling a release application (migration 0050).
//!
//! An artist may withdraw their own application while nothing can have been
//! sent: correction states, staff review, rights hold and READY_FOR_DELIVERY
//! before the agreement is signed (delivery waits for the signature, 0048).
//! Running pipeline states answer `RELEASE_BUSY` so a worker never finds a
//! withdrawn release under a leased job.
//!
//! Artists get [`MONTHLY_LIMIT`] cancellations per organisation per calendar
//! month (KST), counted from `release.withdrawn` audit rows marked `ARTIST`.
//! Beyond that they open an inquiry and staff cancel it; staff cancellations
//! are not counted.
use crate::{
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

pub const MONTHLY_LIMIT: i64 = 3;

/// Who cancels: the artist (counted) or staff on the artist's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Artist,
    Staff,
}

impl By {
    fn code(self) -> &'static str {
        match self {
            By::Artist => "ARTIST",
            By::Staff => "STAFF",
        }
    }
}

const WITHDRAWABLE: &[&str] = &[
    "STAGE1_CORRECTION",
    "STAGE2_REVIEW",
    "STAGE2_CORRECTION",
    "STAGE3_CORRECTION",
    "READY_FOR_DELIVERY",
    "ON_HOLD_RIGHTS",
];
const RUNNING: &[&str] = &[
    "SUBMITTED",
    "STAGE1_RUNNING",
    "STAGE1_PASSED",
    "STAGE2_RUNNING",
    "STAGE2_PASSED",
    "STAGE3_PREPARING",
];

/// Artist cancellations this calendar month (KST).
async fn used(c: &mut PgConnection, org: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events
         WHERE org_id=$1 AND action='release.withdrawn' AND reason_code='ARTIST'
           AND occurred_at >= (date_trunc('month', now() AT TIME ZONE 'Asia/Seoul') AT TIME ZONE 'Asia/Seoul')",
    )
    .bind(org)
    .fetch_one(&mut *c)
    .await?)
}

fn quota_json(used: i64) -> Value {
    json!({"limit": MONTHLY_LIMIT, "used": used, "remaining": (MONTHLY_LIMIT - used).max(0)})
}

/// `GET /api/orgs/{org}/withdrawals`: this month's cancellation quota.
pub async fn quota(pool: &PgPool, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let n = used(&mut tx, org).await?;
    tx.commit().await?;
    Ok(quota_json(n))
}

/// Withdraw inside the caller's transaction. The caller has authorised the
/// actor (artist ACL or staff duty).
pub async fn withdraw(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    by: By,
    actor_user: Uuid,
    request: Uuid,
) -> Result<Value> {
    // One cancellation at a time per organisation keeps the monthly count exact.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('release.withdraw:' || $1::text))")
        .bind(org)
        .execute(&mut *c)
        .await?;
    let row = sqlx::query(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    let status: String = row.get("status");
    if RUNNING.contains(&status.as_str()) {
        return Err(Error::PolicyGate("RELEASE_BUSY"));
    }
    if !WITHDRAWABLE.contains(&status.as_str()) {
        return Err(Error::PolicyGate("RELEASE_NOT_WITHDRAWABLE"));
    }
    let signed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.documents WHERE org_id=$1 AND release_id=$2 AND kind='AGREEMENT' AND status='SIGNED')",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *c)
    .await?;
    if signed {
        // Delivery may already be under way: a takedown goes through staff.
        return Err(Error::PolicyGate("RELEASE_ALREADY_DELIVERING"));
    }
    let used_now = used(c, org).await?;
    if by == By::Artist && used_now >= MONTHLY_LIMIT {
        return Err(Error::PolicyGate("WITHDRAW_LIMIT_REACHED"));
    }
    sqlx::query("SELECT set_config('audeniq.withdraw_by',$1,true)")
        .bind(by.code())
        .execute(&mut *c)
        .await?;
    sqlx::query(
        "UPDATE catalog.releases SET status='WITHDRAWN', row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND status=$3",
    )
    .bind(org)
    .bind(release)
    .bind(&status)
    .execute(&mut *c)
    .await?;
    sqlx::query("SELECT set_config('audeniq.withdraw_by','',true)")
        .execute(&mut *c)
        .await?;
    sqlx::query(
        "UPDATE portal.documents SET status='CANCELLED', row_version=row_version+1, updated_at=now()
         WHERE org_id=$1 AND release_id=$2 AND kind='AGREEMENT' AND status NOT IN ('SIGNED','REJECTED','CANCELLED')",
    )
    .bind(org)
    .bind(release)
    .execute(&mut *c)
    .await?;
    operations::audit(
        c,
        Some(actor_user),
        Some(org),
        Some(release),
        "release.withdrawn",
        by.code(),
        request,
    )
    .await?;
    let used_after = used_now + i64::from(by == By::Artist);
    Ok(json!({"release_id": release, "status": "WITHDRAWN", "quota": quota_json(used_after)}))
}

/// `POST /api/orgs/{org}/releases/{id}/withdraw` by an editor of the release.
pub async fn withdraw_by_artist(
    pool: &PgPool,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> Result<Value> {
    let mut tx = pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let out = withdraw(&mut tx, org, release, By::Artist, a.user, a.request).await?;
    tx.commit().await?;
    Ok(out)
}
