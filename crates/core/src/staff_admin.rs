//! Operator side of the staff portal: grant / revoke staff roles and the
//! DSP registry overview. Runs with the schema-owner login from the admin
//! CLI; the API role can only read `identity.staff_members`.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const ROLES: &[&str] = &["ADMIN", "REVIEWER", "OPERATOR", "SUPPORT"];

pub async fn list(pool: &PgPool) -> Result<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('email',u.email,'user_id',s.user_id,'role',s.role,'status',s.status,
                'granted_by',s.granted_by,'granted_at',s.granted_at,'revoked_at',s.revoked_at)
         FROM identity.staff_members s JOIN identity.users u ON u.id=s.user_id ORDER BY u.email",
    )
    .fetch_all(pool)
    .await?;
    Ok(Value::Array(rows))
}

async fn user_by_email(tx: &mut sqlx::PgConnection, email: &str) -> Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM identity.users WHERE email=lower($1)")
        .bind(email.trim())
        .fetch_optional(&mut *tx)
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
    let mut out = Vec::new();
    for d in crate::dsp_registry::Dsp::ALL {
        let spec = d.spec();
        let row = sqlx::query(
            "SELECT p.delivery_enabled, p.transport, o.stage,
                    ARRAY(SELECT g.requirement FROM execution.partner_onboarding_gaps(p.partner_id) g) AS gaps
             FROM execution.adapter_profiles p LEFT JOIN execution.partner_onboarding o ON o.partner_id=p.partner_id
             WHERE p.partner_id=$1",
        )
        .bind(spec.code)
        .fetch_optional(pool)
        .await?;
        out.push(json!({
            "dsp": spec.code, "name": spec.name, "dsp_id": d.uuid(),
            "delivery_enabled": row.as_ref().map(|r| r.get::<bool,_>("delivery_enabled")),
            "transport": row.as_ref().map(|r| r.get::<String,_>("transport")),
            "stage": row.as_ref().and_then(|r| r.get::<Option<String>,_>("stage")),
            "gaps": row.as_ref().map(|r| r.get::<Vec<String>,_>("gaps")),
        }));
    }
    Ok(Value::Array(out))
}
