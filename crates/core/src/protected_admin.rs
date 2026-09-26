//! Operator management of the protected-artist list (sandbox round 3: the
//! list was editable only with raw schema-owner SQL). Used by the
//! `audeniq-admin protected ...` CLI, which runs with the migration/owner
//! database role; runtime roles have no write grant on these tables.
//! Every change writes `catalog.protected_artist_changes` (operator, op,
//! detail) and an `operations.audit_events` row in the same transaction.
use crate::error::{Error, Result};
use crate::protected_names::{Action, Mode};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

fn mode_str(m: Mode) -> &'static str {
    match m {
        Mode::Contains => "CONTAINS",
        Mode::Token => "TOKEN",
    }
}
fn action_str(a: Action) -> &'static str {
    match a {
        Action::Block => "BLOCK",
        Action::Review => "REVIEW",
    }
}

async fn log(
    c: &mut PgConnection,
    operator: &str,
    op: &str,
    id: Option<Uuid>,
    detail: Value,
) -> Result<()> {
    if operator.trim().is_empty() {
        return Err(Error::Invalid);
    }
    sqlx::query("INSERT INTO catalog.protected_artist_changes(operator, op, protected_artist_id, detail) VALUES($1,$2,$3,$4)")
        .bind(operator)
        .bind(op)
        .bind(id)
        .bind(&detail)
        .execute(&mut *c)
        .await?;
    sqlx::query("INSERT INTO operations.audit_events(id,actor_user_id,actor_service,org_id,resource_id,action,reason_code,request_id) VALUES($1,NULL,$2,NULL,$3,$4,'OPERATOR_CHANGE',$5)")
        .bind(Uuid::new_v4())
        .bind(format!("audeniq-admin:{operator}"))
        .bind(id)
        .bind(format!("protected_artist.{op}"))
        .bind(Uuid::new_v4())
        .execute(&mut *c)
        .await?;
    Ok(())
}

async fn id_of(c: &mut PgConnection, name: &str) -> Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM catalog.protected_artists WHERE name=$1")
        .bind(name)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)
}

/// Add (or reactivate) an entry.
pub async fn add(
    pool: &PgPool,
    operator: &str,
    name: &str,
    mode: Mode,
    action: Action,
    note: Option<&str>,
) -> Result<Uuid> {
    let mut tx = pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO catalog.protected_artists(name, note, match_mode, action) VALUES($1,$2,$3,$4)
         ON CONFLICT(name) DO UPDATE SET active=true, match_mode=EXCLUDED.match_mode, action=EXCLUDED.action,
           note=COALESCE(EXCLUDED.note, catalog.protected_artists.note)
         RETURNING id",
    )
    .bind(name.trim())
    .bind(note)
    .bind(mode_str(mode))
    .bind(action_str(action))
    .fetch_one(&mut *tx)
    .await?;
    log(
        &mut tx,
        operator,
        "add",
        Some(id),
        json!({"name": name, "match_mode": mode_str(mode), "action": action_str(action), "note": note}),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Remove an entry from enforcement (soft: history and exceptions stay).
pub async fn set_active(pool: &PgPool, operator: &str, name: &str, active: bool) -> Result<()> {
    let mut tx = pool.begin().await?;
    let id = id_of(&mut tx, name).await?;
    sqlx::query("UPDATE catalog.protected_artists SET active=$2 WHERE id=$1")
        .bind(id)
        .bind(active)
        .execute(&mut *tx)
        .await?;
    let op = if active { "activate" } else { "deactivate" };
    log(&mut tx, operator, op, Some(id), json!({"name": name})).await?;
    tx.commit().await?;
    Ok(())
}

/// Add or update an alias / signature phrase.
pub async fn add_alias(
    pool: &PgPool,
    operator: &str,
    name: &str,
    alias: &str,
    phrase: bool,
    mode: Mode,
    action: Action,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let id = id_of(&mut tx, name).await?;
    let kind = if phrase { "PHRASE" } else { "NAME" };
    sqlx::query(
        "INSERT INTO catalog.protected_artist_aliases(protected_artist_id, alias, kind, match_mode, action) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(protected_artist_id, alias) DO UPDATE SET kind=EXCLUDED.kind, match_mode=EXCLUDED.match_mode, action=EXCLUDED.action",
    )
    .bind(id)
    .bind(alias.trim())
    .bind(kind)
    .bind(mode_str(mode))
    .bind(action_str(action))
    .execute(&mut *tx)
    .await?;
    log(
        &mut tx,
        operator,
        "alias_add",
        Some(id),
        json!({"name": name, "alias": alias, "kind": kind, "match_mode": mode_str(mode), "action": action_str(action)}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Delete an alias.
pub async fn remove_alias(pool: &PgPool, operator: &str, name: &str, alias: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let id = id_of(&mut tx, name).await?;
    let n = sqlx::query(
        "DELETE FROM catalog.protected_artist_aliases WHERE protected_artist_id=$1 AND alias=$2",
    )
    .bind(id)
    .bind(alias)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(Error::NotFound);
    }
    log(
        &mut tx,
        operator,
        "alias_remove",
        Some(id),
        json!({"name": name, "alias": alias}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Grant (or re-grant) an organization exception, e.g. the artist's label.
pub async fn grant_exception(
    pool: &PgPool,
    operator: &str,
    name: &str,
    org: Uuid,
    reason: &str,
) -> Result<()> {
    if reason.trim().is_empty() {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    let id = id_of(&mut tx, name).await?;
    sqlx::query(
        "INSERT INTO catalog.protected_artist_exceptions(protected_artist_id, org_id, reason, granted_by) VALUES($1,$2,$3,$4)
         ON CONFLICT(protected_artist_id, org_id) DO UPDATE SET reason=EXCLUDED.reason, granted_by=EXCLUDED.granted_by, granted_at=now(), revoked_at=NULL",
    )
    .bind(id)
    .bind(org)
    .bind(reason)
    .bind(operator)
    .execute(&mut *tx)
    .await?;
    log(
        &mut tx,
        operator,
        "exception_grant",
        Some(id),
        json!({"name": name, "org_id": org, "reason": reason}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Revoke an organization exception.
pub async fn revoke_exception(pool: &PgPool, operator: &str, name: &str, org: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    let id = id_of(&mut tx, name).await?;
    let n = sqlx::query("UPDATE catalog.protected_artist_exceptions SET revoked_at=now() WHERE protected_artist_id=$1 AND org_id=$2 AND revoked_at IS NULL")
        .bind(id)
        .bind(org)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(Error::NotFound);
    }
    log(
        &mut tx,
        operator,
        "exception_revoke",
        Some(id),
        json!({"name": name, "org_id": org}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The whole list as JSON (entries with policy, aliases and exceptions).
pub async fn list(pool: &PgPool) -> Result<Value> {
    let rows = sqlx::query(
        "SELECT p.id, p.name, p.active, p.match_mode, p.action, p.note,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('alias',a.alias,'kind',a.kind,'match_mode',a.match_mode,'action',a.action) ORDER BY a.alias)
                            FROM catalog.protected_artist_aliases a WHERE a.protected_artist_id=p.id), '[]'::jsonb) AS aliases,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('org_id',x.org_id,'reason',x.reason,'granted_by',x.granted_by,'revoked',x.revoked_at IS NOT NULL) ORDER BY x.granted_at)
                            FROM catalog.protected_artist_exceptions x WHERE x.protected_artist_id=p.id), '[]'::jsonb) AS exceptions
           FROM catalog.protected_artists p ORDER BY p.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(Value::Array(
        rows.iter()
            .map(|r| {
                json!({
                    "id": r.get::<Uuid, _>("id"),
                    "name": r.get::<String, _>("name"),
                    "active": r.get::<bool, _>("active"),
                    "match_mode": r.get::<String, _>("match_mode"),
                    "action": r.get::<String, _>("action"),
                    "note": r.get::<Option<String>, _>("note"),
                    "aliases": r.get::<Value, _>("aliases"),
                    "exceptions": r.get::<Value, _>("exceptions"),
                })
            })
            .collect(),
    ))
}
