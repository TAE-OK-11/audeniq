//! Organization members in the studio: who is in the workspace, inviting
//! someone by the email of their AUDENIQ account, and the invitee's side
//! (pending invitations, accept or decline).
//!
//! Membership rules stay those of [`crate::catalog::member`]: only an OWNER
//! invites or changes roles, an invitation is accepted only from the
//! invitee's own session, and owners are never changed here.
use crate::{
    api::AppState,
    auth::{self, Actor},
    catalog::{self, MemberInput},
    error::{Error, Result},
    operations,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// Invitations an owner may send per 15 minutes (each one reveals whether
/// an email has an account).
const INVITES_PER_WINDOW: i32 = 20;

/// `a***@example.com` for members who are not owners.
fn masked(email: &str) -> String {
    crate::email::mask_address(email, 1)
}

/// Members of `org`: everyone for an OWNER (invited and revoked included,
/// full emails); ACTIVE members with masked emails for anyone else.
pub async fn list(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, false).await?;
    let owner = role == "OWNER";
    let mut rows: Vec<Value> = sqlx::query_scalar(concat!(
        "SELECT jsonb_build_object('user_id',m.user_id,'role',m.role,'status',m.status,
                'invited_at',m.invited_at,'accepted_at',m.accepted_at,'email',",
        crate::email_ref!("u"),
        ") FROM identity.memberships m JOIN identity.users u ON u.id=m.user_id
          WHERE m.org_id=$1 AND ($2 OR m.status='ACTIVE')
          ORDER BY (m.role='OWNER') DESC, m.status, coalesce(m.accepted_at, m.invited_at)"
    ))
    .bind(org)
    .bind(owner)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    for r in &mut rows {
        crate::pii::reveal(&s.payout_keys, r)?;
        if !owner && let Some(e) = r["email"].as_str().map(masked) {
            r["email"] = json!(e);
        }
        r["me"] = json!(r["user_id"].as_str() == Some(&a.user.to_string()));
    }
    Ok(json!({"role": role, "items": rows}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InviteInput {
    pub email: String,
    pub role: String,
}

/// An OWNER invites the account with this email (it must already exist:
/// the person signs up first). The invitee accepts from their own session.
pub async fn invite(s: &AppState, a: &Actor, org: Uuid, i: InviteInput) -> Result<Value> {
    let email = i.email.trim();
    if email.is_empty() || email.len() > 320 || !email.contains('@') {
        return Err(Error::InvalidCode("MEMBER_EMAIL_INVALID"));
    }
    {
        let mut c = s.pool.acquire().await?;
        if auth::membership(&mut c, a, org, true).await? != "OWNER" {
            return Err(Error::Forbidden);
        }
    }
    auth::rate(&s.pool, &format!("invite:{}", a.user), INVITES_PER_WINDOW).await?;
    let user = crate::pii::user_by_email(&s.payout_keys, &s.pool, email)
        .await?
        .ok_or(Error::PolicyGate("MEMBER_ACCOUNT_NOT_FOUND"))?;
    if user == a.user {
        return Err(Error::PolicyGate("MEMBER_IS_SELF"));
    }
    catalog::member(
        s,
        a,
        org,
        MemberInput {
            user_id: user,
            role: i.role,
            status: "ACTIVE".into(),
        },
    )
    .await
}

/// Invitations waiting for the signed-in user.
pub async fn invitations(s: &AppState, a: &Actor) -> Result<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('org_id',m.org_id,'org_name',o.name,'role',m.role,'invited_at',m.invited_at)
           FROM identity.memberships m JOIN identity.orgs o ON o.id=m.org_id
          WHERE m.user_id=$1 AND m.status='INVITED' ORDER BY m.invited_at DESC",
    )
    .bind(a.user)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": rows}))
}

/// The invitee declines: the invitation is closed (REVOKED, as a revoked
/// member; the owner may invite again).
pub async fn decline(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query(
        "UPDATE identity.memberships SET status='REVOKED' WHERE org_id=$1 AND user_id=$2 AND status='INVITED'",
    )
    .bind(org)
    .bind(a.user)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(Error::NotFound);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(a.user),
        "membership.declined",
        "INVITEE_DECLINE",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"status": "DECLINED"}))
}

/// An OWNER takes back an invitation the invitee has not answered: it is
/// closed like a declined one (REVOKED; the owner may invite again). Active
/// members are not touched here.
pub async fn cancel_invite(s: &AppState, a: &Actor, org: Uuid, user: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    if auth::membership(&mut tx, a, org, true).await? != "OWNER" {
        return Err(Error::Forbidden);
    }
    let n = sqlx::query(
        "UPDATE identity.memberships SET status='REVOKED' WHERE org_id=$1 AND user_id=$2 AND status='INVITED'",
    )
    .bind(org)
    .bind(user)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(Error::PolicyGate("MEMBER_INVITE_NOT_PENDING"));
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(user),
        "membership.invite_cancelled",
        "OWNER_CANCEL",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"user_id": user, "status": "CANCELLED"}))
}

async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list(&s, &a, org).await?))
}
async fn h_invite(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<InviteInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(invite(&s, &a, org, i).await?))
}
async fn h_cancel_invite(
    State(s): State<AppState>,
    Path((org, user)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel_invite(&s, &a, org, user).await?))
}
async fn h_invitations(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(invitations(&s, &a).await?))
}
async fn h_decline(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(decline(&s, &a, org).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/members", get(h_list))
        .route("/api/orgs/{org}/members/invite", post(h_invite))
        .route(
            "/api/orgs/{org}/members/{user}/cancel-invite",
            post(h_cancel_invite),
        )
        .route("/api/me/invitations", get(h_invitations))
        .route("/api/me/invitations/{org}/decline", post(h_decline))
}

#[cfg(test)]
mod tests {
    #[test]
    fn non_owners_see_masked_emails() {
        assert_eq!(super::masked("artist@example.com"), "a***@example.com");
        assert_eq!(super::masked("nope"), "***");
    }
}
