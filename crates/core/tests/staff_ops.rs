//! Staff levels and the admin console's operations (crate::staff_ops):
//! level gates, the OWNER-only staff changes the database itself enforces,
//! step-up codes, and the operations running on the real
//! `audeniq_staff_ops` grants (deploy/grants.sql).
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database, finance,
};
use axum::{Router, http::StatusCode};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use uuid::Uuid;

mod support;
use support::*;

/// A pool that works as `audeniq_staff_ops` with exactly its grants.
async fn ops_pool(pool: &PgPool) -> PgPool {
    // Roles are cluster-wide and the suites run in parallel: one at a time.
    let mut c = pool.acquire().await.unwrap();
    c.execute("SELECT pg_advisory_lock(746001)").await.unwrap();
    c.execute(
        "DO $$ BEGIN
           IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_api') THEN CREATE ROLE audeniq_api NOLOGIN; END IF;
           IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_worker') THEN CREATE ROLE audeniq_worker NOLOGIN; END IF;
           IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_staff_ops') THEN CREATE ROLE audeniq_staff_ops NOLOGIN; END IF;
         END $$",
    )
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(&mut *c)
        .await
        .unwrap();
    c.execute("SELECT pg_advisory_unlock(746001)")
        .await
        .unwrap();
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|c, _| {
            Box::pin(async move {
                c.execute("SET ROLE audeniq_staff_ops").await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap()
}

async fn console(pool: &PgPool, ops: bool) -> Router {
    audeniq_core::config::set_dual_approval(true);
    database::MIGRATOR.run(pool).await.unwrap();
    let mut s = AppState::new(
        pool.clone(),
        Config {
            database_url: "postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2".into(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
            test_only_bypass_dsp_gate: true,
        },
        Arc::new(MemStore::default()),
    )
    .await
    .unwrap();
    if ops {
        s.staff_ops = Some(ops_pool(pool).await);
    }
    router(s)
}

async fn staff(app: &Router, pool: &PgPool, role: &str) -> (User, String) {
    let u = user(app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,$2,'test')")
        .bind(u.user)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let email = audeniq_core::pii::user_email(&keys, pool, u.user)
        .await
        .unwrap();
    (u, email)
}

async fn role_of(pool: &PgPool, user: Uuid) -> Option<(String, String)> {
    sqlx::query_as("SELECT role, status FROM identity.staff_members WHERE user_id=$1")
        .bind(user)
        .fetch_optional(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn levels_decide_what_each_staff_member_may_run(pool: PgPool) {
    let app = console(&pool, true).await;
    let (support, _) = staff(&app, &pool, "SUPPORT").await;
    let (operator, _) = staff(&app, &pool, "OPERATOR").await;
    let (reviewer, _) = staff(&app, &pool, "REVIEWER").await;
    let (admin, _) = staff(&app, &pool, "ADMIN").await;
    let (owner, _) = staff(&app, &pool, "OWNER").await;
    let artist = user(&app).await;

    let (s, me) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&owner)).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert_eq!(
        (me["role"].as_str(), me["level"].as_u64()),
        (Some("OWNER"), Some(4))
    );
    assert!(me["duties"].as_array().unwrap().contains(&json!("FINANCE")));

    let (s, index) = send(&app, "GET", "/api/staff/ops", Value::Null, Some(&operator)).await;
    assert_eq!(s, StatusCode::OK, "{index}");
    assert_eq!(index["available"], true);
    assert_eq!(index["level"], 2);
    let allowed = |v: &Value, area: &str, op: &str| {
        v["ops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["area"] == area && o["op"] == op)
            .unwrap()["allowed"]
            .as_bool()
            .unwrap()
    };
    assert!(allowed(&index, "payouts", "approve"));
    assert!(allowed(&index, "ai-limits", "set"));
    assert!(!allowed(&index, "protected", "add"));
    assert!(!allowed(&index, "staff", "grant"));

    // Non-staff never see the console.
    let (s, _) = send(&app, "GET", "/api/staff/ops", Value::Null, Some(&artist)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // SUPPORT (1) cannot read the protected list (2); REVIEWER can.
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/protected/list",
        Value::Null,
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["code"], "STAFF_LEVEL_REQUIRED");
    assert_eq!(v["error"]["details"]["level"], 2);
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/protected/list",
        Value::Null,
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // Money is the Finance duty, not a level: REVIEWER (2) no, OPERATOR (2) yes.
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/payouts/list",
        Value::Null,
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["details"]["duty"], "FINANCE");
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/payouts/list?status=REQUESTED",
        Value::Null,
        Some(&operator),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // ADMIN (3) manages protected names on the operations login.
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/protected/add",
        json!({"name": "Famous Singer", "action": "REVIEW", "note": "label request"}),
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/protected/add",
        json!({"name": "Famous Singer", "action": "REVIEW", "note": "label request"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/protected/alias",
        json!({"name": "Famous Singer", "alias": "F. Singer"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/protected/grant-exception",
        json!({"name": "Famous Singer", "org_id": artist.org, "reason": "the artist's own label"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = send(
        &app,
        "GET",
        "/api/staff/ops/protected/list",
        Value::Null,
        Some(&admin),
    )
    .await;
    assert!(list.to_string().contains("F. Singer"), "{list}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/protected/add",
        json!({}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["details"]["field"], "name");
    // Reads are GET, changes POST.
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/ops/protected/add",
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // OPERATOR (2) raises an artist's AI limit after an inquiry.
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let email = audeniq_core::pii::user_email(&keys, &pool, artist.user)
        .await
        .unwrap();
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/ai-limits/set",
        json!({"email": email, "limit": 400, "note": "inquiry 12"}),
        Some(&operator),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = send(
        &app,
        "GET",
        "/api/staff/ops/ai-limits/list",
        Value::Null,
        Some(&operator),
    )
    .await;
    assert!(v.to_string().contains("400"), "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/ai-limits/set",
        json!({"email": "nobody@example.test", "limit": 5}),
        Some(&operator),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("ACCOUNT_NOT_FOUND"))
    );

    // ADMIN registers and disables an AI agent; the secret is shown once.
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/ai-clients/add",
        json!({"name": "Partner agent"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["client_secret"].as_str().is_some_and(|s| s.len() > 20));
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/ai-clients/disable",
        json!({"client_id": v["client_id"]}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/ai-clients/list",
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // Reads of the remaining areas work on the operations login.
    for path in [
        "/api/staff/ops/developer-apps/list",
        "/api/staff/ops/developer-apps/list?status=REVIEW",
        "/api/staff/ops/identifiers/list",
        "/api/staff/ops/partners/list",
        "/api/staff/ops/staff/list",
    ] {
        let (s, v) = send(&app, "GET", path, Value::Null, Some(&admin)).await;
        assert_eq!(s, StatusCode::OK, "{path} {v}");
    }
    // Partner onboarding records on the operations login.
    for (op, body) in [
        (
            "contract",
            json!({"partner_id": "merlin", "reference": "MERLIN-2026-01"}),
        ),
        (
            "merlin-eligible",
            json!({"dsp_code": "D-11", "eligible": true}),
        ),
        (
            "dpid",
            json!({"partner_id": "merlin", "dpid": "PADPIDA2011021601U"}),
        ),
    ] {
        let (s, v) = send(
            &app,
            "POST",
            &format!("/api/staff/ops/partners/{op}"),
            body,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{op} {v}");
    }
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/partners/status?partner_id=merlin",
        Value::Null,
        Some(&reviewer),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/identifiers/register",
        json!({"kind": "ISRC", "prefix": "KR-A1B"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "OWNER only: {v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/identifiers/register",
        json!({"kind": "ISRC", "prefix": "KR-A1B"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[sqlx::test]
async fn only_an_owner_changes_staff_and_never_themselves(pool: PgPool) {
    let app = console(&pool, true).await;
    let (owner, owner_email) = staff(&app, &pool, "OWNER").await;
    let (other_owner, other_owner_email) = staff(&app, &pool, "OWNER").await;
    let (admin, _) = staff(&app, &pool, "ADMIN").await;
    let newcomer = user(&app).await;
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let newcomer_email = audeniq_core::pii::user_email(&keys, &pool, newcomer.user)
        .await
        .unwrap();

    // ADMIN (3) cannot reach staff changes at all.
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/grant",
        json!({"email": newcomer_email, "role": "SUPPORT"}),
        Some(&admin),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("STAFF_LEVEL_REQUIRED"))
    );

    // The OWNER grants, changes and revokes a level below their own.
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/grant",
        json!({"email": newcomer_email, "role": "support"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["previous"], Value::Null);
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/grant",
        json!({"email": newcomer_email, "role": "ADMIN"}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["previous"], "SUPPORT");
    assert_eq!(
        role_of(&pool, newcomer.user).await,
        Some(("ADMIN".into(), "ACTIVE".into()))
    );
    let (_, list) = send(
        &app,
        "GET",
        "/api/staff/ops/staff/list",
        Value::Null,
        Some(&owner),
    )
    .await;
    assert_eq!(list[0]["role"], "OWNER", "highest level first: {list}");
    assert!(list.to_string().contains(&newcomer_email));

    // Never OWNER, never oneself, never another OWNER (CLI only).
    for (body, code) in [
        (
            json!({"email": newcomer_email, "role": "OWNER"}),
            "STAFF_ROLE_INVALID",
        ),
        (
            json!({"email": owner_email, "role": "ADMIN"}),
            "STAFF_SELF_CHANGE",
        ),
        (
            json!({"email": other_owner_email, "role": "ADMIN"}),
            "STAFF_OWNER_PROTECTED",
        ),
    ] {
        let (_, v) = send(
            &app,
            "POST",
            "/api/staff/ops/staff/grant",
            body,
            Some(&owner),
        )
        .await;
        assert_eq!(v["error"]["code"], code, "{v}");
    }
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/revoke",
        json!({"user_id": other_owner.user}),
        Some(&owner),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_OWNER_PROTECTED");
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/reset-mfa",
        json!({"user_id": owner.user}),
        Some(&owner),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_SELF_CHANGE");

    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/reset-mfa",
        json!({"user_id": newcomer.user}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/revoke",
        json!({"user_id": newcomer.user}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(role_of(&pool, newcomer.user).await.unwrap().1, "REVOKED");
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/ops/staff/revoke",
        json!({"user_id": newcomer.user}),
        Some(&owner),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_NOT_FOUND");

    // The database holds the rule even for a caller that skips the API: the
    // operations login cannot write staff rows, and the function checks the
    // actor's own level.
    let ops = ops_pool(&pool).await;
    let e = sqlx::query("UPDATE identity.staff_members SET role='OWNER' WHERE user_id=$1")
        .bind(admin.user)
        .execute(&ops)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("permission denied"), "{e}");
    let e = sqlx::query("SELECT identity.staff_set($1,$1,'ADMIN','x')")
        .bind(admin.user)
        .execute(&ops)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("STAFF_OWNER_REQUIRED"), "{e}");
    let e = sqlx::query("DELETE FROM identity.staff_mfa")
        .execute(&ops)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("permission denied"), "{e}");
}

#[sqlx::test]
async fn money_needs_two_people_and_a_fresh_code(pool: PgPool) {
    // Safety: this test binary only sets this value.
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
    }
    let app = console(&pool, true).await;
    let (approver, _) = staff(&app, &pool, "OPERATOR").await;
    let (executor, _) = staff(&app, &pool, "ADMIN").await;
    let org = approver.org;
    let dec = |s: &str| s.parse::<Decimal>().unwrap();
    finance::post_transaction(
        &pool,
        org,
        finance::PostTransaction {
            transaction_code: "ROYALTY_ACCRUAL",
            currency: "KRW",
            entries: vec![
                finance::LedgerEntryInput {
                    account: "ROYALTY_RECEIVABLE",
                    side: finance::EntrySide::Debit,
                    amount: dec("100000"),
                    currency: None,
                    party_id: None,
                    isrc: None,
                    split_snapshot_id: None,
                },
                finance::LedgerEntryInput {
                    account: "ROYALTY_PAYABLE",
                    side: finance::EntrySide::Credit,
                    amount: dec("100000"),
                    currency: None,
                    party_id: None,
                    isrc: None,
                    split_snapshot_id: None,
                },
            ],
            match_status: Some("AUTO"),
            fx_rate: None,
            fee_policy_version: Some("fee-2026-01"),
            contract_version: Some("ctr-7"),
            tax_rule_version: Some("tax-2026-kr"),
            source_ref: json!({"report_line": "synthetic"}),
            description: "synthetic accrual",
            created_by: None,
        },
    )
    .await
    .unwrap();
    let (order, _) = finance::create_payout_order(
        &pool,
        org,
        finance::PayoutOrderInput {
            idempotency_key: "ops-0001",
            payee_party_id: Uuid::nil(),
            amount: dec("30000"),
            currency: "KRW",
            created_by: None,
        },
    )
    .await
    .unwrap();

    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/approve",
        json!({"order_id": order}),
        Some(&approver),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // The approver cannot also pay it out.
    let receipt = json!({"transfer_receipt": "IBK 이체확인증"});
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/manual",
        json!({"order_id": order, "bank_tx": "IBK-1", "evidence": receipt}),
        Some(&approver),
    )
    .await;
    assert_eq!(v["error"]["code"], "PAYOUT_EXECUTOR_MUST_DIFFER", "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/manual",
        json!({"order_id": order, "bank_tx": "IBK-1", "evidence": receipt}),
        Some(&executor),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(&app, "POST", "/api/staff/ops/payouts/return", json!({"order_id": order, "bank_reference": "IBK-RET-1", "amount": "30000", "evidence": {"statement": "returned"}}), Some(&executor)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        finance::get_payout_status(&pool, org, order).await.unwrap(),
        "RETURNED"
    );

    // Royalty reports on the operations login.
    let csv =
        "isrc,dsp_track_id,quantity,gross_amount,currency\nXXYYY9900001,DSP999,100,3.00,USD\n";
    let (s, v) = send(&app, "POST", "/api/staff/ops/royalty/import", json!({"org_id": org, "dsp_id": "mockdsp", "period_start": "2026-08-01", "period_end": "2026-08-31", "currency": "USD", "filename": "aug.csv", "content": csv}), Some(&executor)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let report = v["report_id"].as_str().unwrap().to_string();
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/royalty/match",
        json!({"org_id": org, "report_id": report}),
        Some(&executor),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(
        &app,
        "GET",
        &format!("/api/staff/ops/royalty/show?org_id={org}&report_id={report}"),
        Value::Null,
        Some(&executor),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["unresolved"], 1);

    // Once the approver has an authenticator, money needs a fresh code.
    let (_, e) = send(
        &app,
        "POST",
        "/api/staff/mfa/enroll",
        json!({}),
        Some(&approver),
    )
    .await;
    let secret = e["secret"].as_str().unwrap().to_string();
    let code = |offset: i64| {
        let raw = audeniq_core::staff_mfa::base32_decode(&secret).unwrap();
        let step = chrono::Utc::now().timestamp() / 30 + offset;
        format!("{:06}", audeniq_core::staff_mfa::hotp(&raw, step as u64))
    };
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/mfa/verify",
        json!({"code": code(0)}),
        Some(&approver),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (second, _) = finance::create_payout_order(
        &pool,
        org,
        finance::PayoutOrderInput {
            idempotency_key: "ops-0002",
            payee_party_id: Uuid::nil(),
            amount: dec("1000"),
            currency: "KRW",
            created_by: None,
        },
    )
    .await
    .unwrap();
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/approve",
        json!({"order_id": second}),
        Some(&approver),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_STEP_UP_REQUIRED", "{v}");
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/approve",
        json!({"order_id": second, "code": "000000"}),
        Some(&approver),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_MFA_CODE_INVALID", "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/approve",
        json!({"order_id": second, "code": code(1)}),
        Some(&approver),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // Cancelling an unsent order is not a money movement: no code.
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/payouts/cancel",
        json!({"order_id": second}),
        Some(&approver),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[sqlx::test]
async fn without_the_operations_login_the_console_points_to_the_cli(pool: PgPool) {
    let app = console(&pool, false).await;
    let (admin, _) = staff(&app, &pool, "ADMIN").await;
    let (_, index) = send(&app, "GET", "/api/staff/ops", Value::Null, Some(&admin)).await;
    assert_eq!(index["available"], false);
    let (s, v) = send(
        &app,
        "GET",
        "/api/staff/ops/protected/list",
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("STAFF_OPS_UNAVAILABLE")
        )
    );
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/protected/add",
        json!({"name": "X"}),
        Some(&admin),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("STAFF_OPS_UNAVAILABLE")
        )
    );
}

#[sqlx::test]
async fn only_an_owner_adds_settlement_balance(pool: PgPool) {
    let app = console(&pool, true).await;
    let (owner, _) = staff(&app, &pool, "OWNER").await;
    let (admin, _) = staff(&app, &pool, "ADMIN").await;
    let artist = user(&app).await;
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let email = audeniq_core::pii::user_email(&keys, &pool, artist.user)
        .await
        .unwrap();
    let credit = json!({"org_id": artist.org, "amount": "50000", "title": "9월 정산금", "reason": "DSP 리포트 수기 반영"});

    // An ADMIN (level 3) neither sees nor adds.
    let (s, v) = send(
        &app,
        "GET",
        &format!("/api/staff/ops/balance/show?email={email}"),
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/balance/credit",
        credit.clone(),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["code"], "STAFF_LEVEL_REQUIRED");

    // Bad amounts never reach the ledger.
    for amount in ["0", "-1", "10.5", "10000001"] {
        let mut b = credit.clone();
        b["amount"] = json!(amount);
        let (_, v) = send(
            &app,
            "POST",
            "/api/staff/ops/balance/credit",
            b,
            Some(&owner),
        )
        .await;
        assert_eq!(v["error"]["code"], "OPS_ARGUMENT_INVALID", "{amount} {v}");
    }

    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/ops/balance/credit",
        credit,
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["withdrawable"], "50000");
    let mut c = pool.acquire().await.unwrap();
    assert_eq!(
        finance::withdrawable(&mut c, artist.org, None)
            .await
            .unwrap(),
        "50000".parse::<Decimal>().unwrap()
    );
    let (title, by): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT description, created_by FROM finance.ledger_transactions WHERE org_id=$1 AND transaction_code='MANUAL_CREDIT'",
    )
    .bind(artist.org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((title.as_str(), by), ("9월 정산금", Some(owner.user)));
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action='balance.credit' AND org_id=$1 AND actor_user_id=$2")
        .bind(artist.org)
        .bind(owner.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audited, 1);

    let (s, v) = send(
        &app,
        "GET",
        &format!("/api/staff/ops/balance/show?email={email}"),
        Value::Null,
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["orgs"][0]["withdrawable"], "50000", "{v}");
    assert_eq!(v["orgs"][0]["credits"][0]["title"], "9월 정산금", "{v}");

    // The database itself refuses anyone but an active OWNER, even on the operations login.
    let ops = ops_pool(&pool).await;
    let e = sqlx::query("SELECT finance.staff_credit_balance($1,$2,1000,'x','y')")
        .bind(admin.user)
        .bind(artist.org)
        .execute(&ops)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("STAFF_OWNER_REQUIRED"), "{e}");
}
