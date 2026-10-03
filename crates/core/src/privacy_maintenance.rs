//! Bounded owner-only maintenance. Never age-delete contracts, finance or assets.
use crate::{
    error::{Error, Result},
    operations,
    payout_keys::KeyRing,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

async fn owner(pool: &PgPool, operator: &str) -> Result<()> {
    if operator.trim().is_empty() || operator.chars().count() > 120 {
        return Err(Error::Invalid);
    }
    crate::text_policy::check(operator)?;
    let owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_roles r ON r.oid=c.relowner WHERE n.nspname='identity' AND c.relname='sessions' AND r.rolname=current_user) OR (SELECT rolsuper FROM pg_roles WHERE rolname=current_user)")
        .fetch_one(pool).await?;
    if !owner {
        return Err(Error::Forbidden);
    }
    Ok(())
}

/// A 24-hour recovery grace is an operational setting, not a statutory period.
pub async fn purge_transient(pool: &PgPool, operator: &str) -> Result<Value> {
    owner(pool, operator).await?;
    let mut tx = pool.begin().await?;
    let sessions=sqlx::query("WITH old AS (SELECT token_hash FROM identity.sessions WHERE expires_at<now()-interval '1 day' OR revoked_at<now()-interval '1 day' ORDER BY expires_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.sessions s USING old WHERE s.token_hash=old.token_hash")
        .execute(&mut *tx).await?.rows_affected();
    let limits=sqlx::query("WITH old AS (SELECT bucket_hash FROM identity.auth_limits WHERE window_start<now()-interval '1 day' ORDER BY window_start LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.auth_limits a USING old WHERE a.bucket_hash=old.bucket_hash")
        .execute(&mut *tx).await?.rows_affected();
    let logs=sqlx::query("WITH old AS (SELECT id FROM privacy.staff_access_logs WHERE retain_until<now() ORDER BY occurred_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM privacy.staff_access_logs l USING old WHERE l.id=old.id")
        .execute(&mut *tx).await?.rows_affected();
    operations::audit(
        &mut tx,
        None,
        None,
        None,
        "privacy.transient.purged",
        &format!(
            "OPERATOR:{} sessions={sessions} limits={limits} expired_logs={logs}",
            operator.trim()
        ),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"sessions":sessions,"auth_limits":limits,"expired_access_logs":logs,"batch_limit":10000}),
    )
}

/// Idempotent 100-row batch. Historical keys must stay available until rows and
/// retained backups have been migrated or expired. No clear numbers leave here.
pub async fn reencrypt_accounts(pool: &PgPool, operator: &str) -> Result<Value> {
    owner(pool, operator).await?;
    let keys = KeyRing::from_env(crate::config::kms_enabled()?)?;
    let mut tx = pool.begin().await?;
    let rows=sqlx::query("SELECT org_id,account_cipher FROM portal.payout_accounts WHERE key_version<>$1 OR substring(account_cipher FROM 1 FOR 6)<>$2 ORDER BY org_id LIMIT 100 FOR UPDATE SKIP LOCKED")
        .bind(keys.active_version()).bind(crate::payout_keys::TAG).fetch_all(&mut *tx).await?;
    for row in &rows {
        let org: Uuid = row.get("org_id");
        let cipher: Vec<u8> = row.get("account_cipher");
        let number = keys.open(org, &cipher)?;
        let cipher = keys.seal(org, &number)?;
        sqlx::query("UPDATE portal.payout_accounts SET account_cipher=$2,key_version=$3,row_version=row_version+1 WHERE org_id=$1")
            .bind(org).bind(cipher).bind(keys.active_version()).execute(&mut *tx).await?;
        operations::audit(
            &mut tx,
            None,
            Some(org),
            Some(org),
            "privacy.account.reencrypted",
            &format!(
                "OPERATOR:{} key_version={}",
                operator.trim(),
                keys.active_version()
            ),
            Uuid::new_v4(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(json!({"reencrypted":rows.len(),"active_version":keys.active_version(),"batch_limit":100}))
}
