//! Two-person approval switched off (`DUAL_APPROVAL_ENABLED=false`): every
//! decision is one person's, including requests filed while it was on.
//! Own test binary: the switch is process-wide.
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod support;
use support::*;

/// A submitted release and its revision (no pipeline run needed).
async fn submitted(app: &axum::Router, pool: &PgPool, store: &MemStore, u: &User) -> (Uuid, Uuid) {
    let mut audio = wav_bytes().to_vec();
    audio.extend_from_slice(&Uuid::new_v4().as_u128().to_le_bytes());
    let asset = register_asset(pool, store, u, "t.wav", &audio).await;
    let release = build_submittable(app, pool, u, asset).await;
    consent_and_submit(app, u, release, &format!("k-dual-{}", Uuid::new_v4())).await;
    let revision: Uuid =
        sqlx::query_scalar("SELECT current_revision_id FROM catalog.releases WHERE id=$1")
            .bind(release)
            .fetch_one(pool)
            .await
            .unwrap();
    (release, revision)
}

#[sqlx::test(migrations = false)]
async fn leftover_requests_close_with_one_person_when_switched_off(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let owner = user(&app).await;
    let (release, revision) = submitted(&app, &pool, &store, &owner).await;

    // Filed while two-person approval was on.
    let member_request = Uuid::new_v4();
    sqlx::query("INSERT INTO rights.override_requests(id,org_id,revision_id,check_code,original_status,proposed_status,reason,requested_by) VALUES($1,$2,$3,'S2_CATALOG_IDENTIFIERS','REVIEW_REQUIRED','PASS','identifiers verified',$4)")
        .bind(member_request).bind(owner.org).bind(revision).bind(owner.user)
        .execute(&pool).await.unwrap();
    let approve = format!(
        "/api/orgs/{}/reviews/overrides/{member_request}/approve",
        owner.org
    );
    let (s, v) = send(&app, "POST", &approve, json!({}), Some(&owner)).await;
    assert_eq!(v["error"]["code"], "SECOND_APPROVER_MUST_DIFFER", "{s} {v}");

    audeniq_core::config::set_dual_approval(false);
    let (s, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&owner)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "not staff: {v}");

    // The OWNER who asked now applies it alone, recorded without a second approver.
    let (s, v) = send(&app, "POST", &approve, json!({}), Some(&owner)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "APPLIED");
    let (actor, second): (Uuid, Option<Uuid>) = sqlx::query_as(
        "SELECT actor_user_id, second_approver_user_id FROM rights.review_overrides WHERE id=$1",
    )
    .bind(Uuid::parse_str(v["override_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((actor, second), (owner.user, None));
    let reason: String = sqlx::query_scalar("SELECT reason_code FROM operations.audit_events WHERE action='override.approved' AND actor_user_id=$1")
        .bind(owner.user).fetch_one(&pool).await.unwrap();
    assert_eq!(reason, "SELF_APPROVED_DUAL_OFF");

    // An EDITOR still cannot PASS alone (same as a direct PASS while off).
    let editor = user(&app).await;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role,status,accepted_at) VALUES($1,$2,'EDITOR','ACTIVE',now()-interval '30 days')")
        .bind(owner.org).bind(editor.user).execute(&pool).await.unwrap();
    // Access to the release, as a member the owner shared it with.
    sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action) VALUES($1,$2,$3,'read'),($1,$2,$3,'write')")
        .bind(owner.org).bind(release).bind(editor.party).execute(&pool).await.unwrap();
    let editor_request = Uuid::new_v4();
    sqlx::query("INSERT INTO rights.override_requests(id,org_id,revision_id,check_code,original_status,proposed_status,reason,requested_by) VALUES($1,$2,$3,'S2_DUPLICATE_RECORDING','REVIEW_REQUIRED','PASS','not a duplicate',$4)")
        .bind(editor_request).bind(owner.org).bind(revision).bind(editor.user)
        .execute(&pool).await.unwrap();
    let (s, v) = send(
        &app,
        "POST",
        &format!(
            "/api/orgs/{}/reviews/overrides/{editor_request}/approve",
            owner.org
        ),
        json!({}),
        Some(&editor),
    )
    .await;
    assert_eq!(v["error"]["code"], "SECOND_APPROVER_MUST_DIFFER", "{s} {v}");

    // A staff 2nd-approval request may be closed by the reviewer who filed
    // it: the table no longer refuses decided_by = requested_by.
    let staff_request = Uuid::new_v4();
    sqlx::query("INSERT INTO rights.staff_approvals(id,org_id,release_id,revision_id,check_codes,reason,requested_by) VALUES($1,$2,$3,$4,ARRAY['S2_SPECIAL_FLAGS'],'explicit marking verified',$5)")
        .bind(staff_request).bind(owner.org).bind(release).bind(revision).bind(owner.user)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE rights.staff_approvals SET status='APPROVED', decided_by=requested_by, decided_at=now() WHERE id=$1")
        .bind(staff_request)
        .execute(&pool)
        .await
        .expect("one person may close it");
}
