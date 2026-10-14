//! Workspace members in the studio: an owner invites an existing account by
//! its email, the invitee accepts (or declines) from their own session, and
//! only owners see full emails and pending invitations.
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

mod support;
use support::*;

async fn email_of(app: &axum::Router, u: &User) -> String {
    let (s, me) = call(app, "GET", "/api/me", Value::Null, Some(u)).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    me["email"].as_str().expect("email on /api/me").to_string()
}

#[sqlx::test]
async fn owners_invite_by_email_and_invitees_decide(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let owner = user(&app).await;
    let invitee = user(&app).await;
    let other = user(&app).await;
    let members = format!("/api/orgs/{}/members", owner.org);
    let invite = format!("{members}/invite");
    let invitee_email = email_of(&app, &invitee).await;

    let (s, v) = call(
        &app,
        "POST",
        &invite,
        json!({"email": "nobody@example.test", "role": "EDITOR"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "MEMBER_ACCOUNT_NOT_FOUND");
    let (s, _) = call(
        &app,
        "POST",
        &invite,
        json!({"email": invitee_email, "role": "OWNER"}),
        Some(&owner),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "owners are not made by invitation"
    );
    // Email case does not matter.
    let (s, v) = call(
        &app,
        "POST",
        &invite,
        json!({"email": invitee_email.to_uppercase(), "role": "EDITOR"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "INVITED");

    // Not a member until accepted: the workspace stays closed to them.
    let (s, _) = call(&app, "GET", &members, Value::Null, Some(&invitee)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, inv) = call(
        &app,
        "GET",
        "/api/me/invitations",
        Value::Null,
        Some(&invitee),
    )
    .await;
    assert_eq!(inv["items"][0]["org_id"], owner.org.to_string(), "{inv}");
    assert_eq!(inv["items"][0]["role"], "EDITOR");
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/memberships/accept", owner.org),
        json!({}),
        Some(&invitee),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, inv) = call(
        &app,
        "GET",
        "/api/me/invitations",
        Value::Null,
        Some(&invitee),
    )
    .await;
    assert_eq!(inv["items"].as_array().unwrap().len(), 0);

    // The owner sees full emails; a member sees masked ones and cannot invite.
    let (_, list) = call(&app, "GET", &members, Value::Null, Some(&owner)).await;
    assert_eq!(list["role"], "OWNER");
    let joined = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["user_id"] == invitee.user.to_string())
        .unwrap()
        .clone();
    assert_eq!(joined["status"], "ACTIVE");
    assert_eq!(joined["email"], invitee_email);
    let (_, list) = call(&app, "GET", &members, Value::Null, Some(&invitee)).await;
    let me = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["me"] == true)
        .unwrap()
        .clone();
    assert!(me["email"].as_str().unwrap().contains("***@"), "{me}");
    let other_email = email_of(&app, &other).await;
    let (s, _) = call(
        &app,
        "POST",
        &invite,
        json!({"email": other_email, "role": "VIEWER"}),
        Some(&invitee),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Declining closes the invitation; it can be sent again.
    let (s, _) = call(
        &app,
        "POST",
        &invite,
        json!({"email": other_email, "role": "VIEWER"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let decline = format!("/api/me/invitations/{}/decline", owner.org);
    let (s, _) = call(&app, "POST", &decline, json!({}), Some(&other)).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&app, "POST", &decline, json!({}), Some(&other)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/memberships/accept", owner.org),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "a declined invitation cannot be accepted"
    );
    let (s, v) = call(
        &app,
        "POST",
        &invite,
        json!({"email": other_email, "role": "VIEWER"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "INVITED");
}
