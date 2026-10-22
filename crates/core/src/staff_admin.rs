//! Operator side of the staff portal: grant / revoke staff roles and the
//! DSP registry overview. Runs with the schema-owner login from the admin
//! CLI; the API role can only read `identity.staff_members`.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const ROLES: &[&str] = &["OWNER", "ADMIN", "REVIEWER", "OPERATOR", "SUPPORT"];

pub async fn list(pool: &PgPool) -> Result<Value> {
    list_with(pool, &crate::pii::cli_keys()?).await
}

/// Every staff row, highest level first (the CLI and the admin console).
pub async fn list_with(pool: &PgPool, keys: &crate::payout_keys::KeyRing) -> Result<Value> {
    let mut rows: Vec<Value> = sqlx::query_scalar(concat!(
        "SELECT jsonb_build_object('email',",
        crate::email_ref!("u"),
        ",'user_id',s.user_id,'role',s.role,'status',s.status,
                'granted_by',s.granted_by,'granted_at',s.granted_at,'revoked_at',s.revoked_at)
         FROM identity.staff_members s JOIN identity.users u ON u.id=s.user_id"
    ))
    .fetch_all(pool)
    .await?;
    for row in &mut rows {
        crate::pii::reveal(keys, row)?;
    }
    let level = |v: &Value| {
        v["role"]
            .as_str()
            .and_then(crate::staff::StaffRole::parse)
            .map_or(0, |r| r.level())
    };
    rows.sort_by(|a, b| {
        (b["status"] == "ACTIVE", level(b))
            .cmp(&(a["status"] == "ACTIVE", level(a)))
            .then(a["email"].as_str().cmp(&b["email"].as_str()))
    });
    Ok(Value::Array(rows))
}

async fn user_by_email(tx: &mut sqlx::PgConnection, email: &str) -> Result<Uuid> {
    crate::pii::user_by_email(&crate::pii::cli_keys()?, &mut *tx, email)
        .await?
        .ok_or(Error::NotFound)
}

pub async fn grant(pool: &PgPool, operator: &str, email: &str, role: &str) -> Result<()> {
    if operator.trim().is_empty() || !ROLES.contains(&role) {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    let user = user_by_email(&mut tx, email).await?;
    sqlx::query(
        "INSERT INTO identity.staff_members(user_id, role, granted_by) VALUES($1,$2,$3)
         ON CONFLICT(user_id) DO UPDATE SET role=EXCLUDED.role, status='ACTIVE', revoked_at=NULL,
           granted_by=EXCLUDED.granted_by, granted_at=now()",
    )
    .bind(user)
    .bind(role)
    .bind(operator.trim())
    .execute(&mut *tx)
    .await?;
    crate::operations::audit(
        &mut tx,
        None,
        None,
        Some(user),
        "staff.granted",
        &format!("OPERATOR:{} {role}", operator.trim()),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn revoke(pool: &PgPool, operator: &str, email: &str) -> Result<()> {
    if operator.trim().is_empty() {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    let user = user_by_email(&mut tx, email).await?;
    let n = sqlx::query(
        "UPDATE identity.staff_members SET status='REVOKED', revoked_at=now() WHERE user_id=$1 AND status='ACTIVE'",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(Error::NotFound);
    }
    crate::operations::audit(
        &mut tx,
        None,
        None,
        Some(user),
        "staff.revoked",
        &format!("OPERATOR:{}", operator.trim()),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Registry + each D-n direct route's onboarding state (owner login: reads
/// the operator-owned onboarding table directly).
pub async fn dsp_overview(pool: &PgPool) -> Result<Value> {
    let codes: Vec<&str> = crate::dsp_registry::Dsp::ALL
        .iter()
        .map(|d| d.spec().code)
        .collect();
    let mut rows: std::collections::HashMap<String, sqlx::postgres::PgRow> = sqlx::query(
        "SELECT p.partner_id, p.delivery_enabled, p.transport, o.stage,
                ARRAY(SELECT g.requirement FROM execution.partner_onboarding_gaps(p.partner_id) g) AS gaps
         FROM execution.adapter_profiles p LEFT JOIN execution.partner_onboarding o ON o.partner_id=p.partner_id
         WHERE p.partner_id=ANY($1)",
    )
    .bind(&codes)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| (r.get("partner_id"), r))
    .collect();
    let out = crate::dsp_registry::Dsp::ALL
        .iter()
        .map(|d| {
            let spec = d.spec();
            let row = rows.remove(spec.code);
            json!({
                "dsp": spec.code, "name": spec.name, "dsp_id": d.uuid(),
                "delivery_enabled": row.as_ref().map(|r| r.get::<bool,_>("delivery_enabled")),
                "transport": row.as_ref().map(|r| r.get::<String,_>("transport")),
                "stage": row.as_ref().and_then(|r| r.get::<Option<String>,_>("stage")),
                "gaps": row.as_ref().map(|r| r.get::<Vec<String>,_>("gaps")),
            })
        })
        .collect();
    Ok(Value::Array(out))
}
