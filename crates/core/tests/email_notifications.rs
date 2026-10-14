//! Notification emails: a notice queues one email per recipient, the API
//! sends it through the first provider that accepts it, retries, gives up,
//! and respects opt-outs — all through the runtime API role's grants.
mod support;

use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    email::{self, Mailer},
    payout_keys::KeyRing,
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use support::*;
use uuid::Uuid;

async fn api_role_app(pool: &PgPool) -> (axum::Router, PgPool) {
    database::MIGRATOR.run(pool).await.unwrap();
    sqlx::raw_sql(
        "DO $$ BEGIN
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_api') THEN CREATE ROLE audeniq_api NOLOGIN; END IF;
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_worker') THEN CREATE ROLE audeniq_worker NOLOGIN; END IF;
         END $$;",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(pool)
        .await
        .unwrap();
    let api_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE audeniq_api").execute(c).await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let state = AppState::new(
        api_pool.clone(),
        Config {
            database_url: "unused".into(),
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
    (router(state), api_pool)
}

async fn notice(pool: &PgPool, org: Uuid, user: Option<Uuid>, title: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO portal.notifications(org_id,user_id,kind,title,detail,link)
         VALUES($1,$2,'RELEASE',$3,'검토가 끝났어요.','/releases') RETURNING id",
    )
    .bind(org)
    .bind(user)
    .bind(title)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn row(
    pool: &PgPool,
    notice: Uuid,
    user: Uuid,
) -> (String, i32, Option<String>, Option<String>) {
    let r = sqlx::query(
        "SELECT status, attempts, provider, last_error FROM portal.notification_emails WHERE notification_id=$1 AND user_id=$2",
    )
    .bind(notice)
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap();
    (r.get(0), r.get(1), r.get(2), r.get(3))
}

fn mailer(providers: &str) -> Mailer {
    unsafe {
        std::env::set_var("EMAIL_FROM", "AUDENIQ <no-reply@audeniq.test>");
        std::env::set_var("EMAIL_PROVIDERS", providers);
    }
    Mailer::from_env(ORIGIN).unwrap().unwrap()
}

#[sqlx::test]
async fn notices_are_emailed_with_failover_retry_and_opt_out(pool: PgPool) {
    provider_configuration_is_checked_at_startup();
    let (app, api_pool) = api_role_app(&pool).await;
    let keys = KeyRing::from_env(false).unwrap();
    let owner = user(&app).await;
    let member = user(&app).await;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role,status) VALUES($1,$2,'EDITOR','ACTIVE')")
        .bind(owner.org)
        .bind(member.user)
        .execute(&pool)
        .await
        .unwrap();
    let owner_email = audeniq_core::pii::user_email(&keys, &pool, owner.user)
        .await
        .unwrap();
    let member_email = audeniq_core::pii::user_email(&keys, &pool, member.user)
        .await
        .unwrap();

    let (s, v) = call(
        &app,
        "GET",
        "/api/me/notification-settings",
        json!({}),
        Some(&member),
    )
    .await;
    assert_eq!(
        (s, v["email_enabled"].as_bool()),
        (StatusCode::OK, Some(true))
    );

    // An org-wide notice queues one email per active member.
    let first = notice(&pool, owner.org, None, "발매가 승인됐어요").await;
    assert_eq!(row(&pool, first, owner.user).await.0, "PENDING");
    assert_eq!(row(&pool, first, member.user).await.0, "PENDING");

    // Turning email off skips what is still queued and stops new ones.
    let (s, _) = call(
        &app,
        "PUT",
        "/api/me/notification-settings",
        json!({"email_enabled": false}),
        Some(&member),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        row(&pool, first, member.user).await,
        ("SKIPPED".into(), 0, None, Some("EMAIL_OPTED_OUT".into()))
    );
    let second = notice(&pool, owner.org, None, "정산이 확정됐어요").await;
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM portal.notification_emails WHERE notification_id=$1",
    )
    .bind(second)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(queued, 1, "only the owner still receives email");
    // A notice addressed to one user goes only to that user.
    let personal = notice(&pool, owner.org, Some(owner.user), "문의에 답변이 달렸어요").await;
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM portal.notification_emails WHERE notification_id=$1",
    )
    .bind(personal)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(queued, 1);

    // The first provider fails, the second one sends (as the API role).
    let m = mailer("test-fail,test");
    let sent = email::dispatch_once(&api_pool, &keys, &m).await.unwrap();
    assert_eq!(sent, 3);
    for n in [first, second, personal] {
        assert_eq!(
            row(&pool, n, owner.user).await,
            ("SENT".into(), 1, Some("test".into()), None)
        );
    }
    let inbox = email::test_sent_to(&owner_email);
    assert_eq!(inbox.len(), 3);
    let mail = inbox
        .iter()
        .find(|m| m.subject == "[AUDENIQ] 발매가 승인됐어요")
        .unwrap();
    assert!(
        mail.text.contains("검토가 끝났어요.") && mail.text.contains(&format!("{ORIGIN}/releases"))
    );
    assert!(email::test_sent_to(&member_email).is_empty());
    // Nothing is sent twice.
    assert_eq!(email::dispatch_once(&api_pool, &keys, &m).await.unwrap(), 0);

    // Every provider failing: retried later, then given up.
    let failing = mailer("test-fail");
    let third = notice(&pool, owner.org, Some(owner.user), "서류를 확인해 주세요").await;
    email::dispatch_once(&api_pool, &keys, &failing)
        .await
        .unwrap();
    assert_eq!(
        row(&pool, third, owner.user).await,
        ("PENDING".into(), 1, None, Some("EMAIL_SEND_FAILED".into()))
    );
    let later: bool = sqlx::query_scalar(
        "SELECT next_attempt_at > now() + interval '50 seconds' FROM portal.notification_emails WHERE notification_id=$1",
    )
    .bind(third)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(later, "a failed send waits before the next try");
    assert_eq!(
        email::dispatch_once(&api_pool, &keys, &failing)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE portal.notification_emails SET next_attempt_at=now(), attempts=$2 WHERE notification_id=$1")
        .bind(third)
        .bind(email::MAX_ATTEMPTS - 1)
        .execute(&pool)
        .await
        .unwrap();
    email::dispatch_once(&api_pool, &keys, &failing)
        .await
        .unwrap();
    assert_eq!(
        row(&pool, third, owner.user).await,
        (
            "FAILED".into(),
            email::MAX_ATTEMPTS,
            None,
            Some("EMAIL_SEND_FAILED".into())
        )
    );

    // A notice too old to be useful is not mailed.
    let old = notice(&pool, owner.org, Some(owner.user), "오래된 알림").await;
    sqlx::query("UPDATE portal.notifications SET created_at=now() - interval '4 days' WHERE id=$1")
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();
    email::dispatch_once(&api_pool, &keys, &m).await.unwrap();
    assert_eq!(row(&pool, old, owner.user).await.0, "SKIPPED");
    assert!(
        !email::test_sent_to(&owner_email)
            .iter()
            .any(|m| m.subject.contains("오래된 알림"))
    );
    email::sweep(&api_pool).await.unwrap();
}

/// Run first in the one test of this binary: the environment is process-wide.
fn provider_configuration_is_checked_at_startup() {
    unsafe {
        std::env::set_var("EMAIL_FROM", "AUDENIQ <no-reply@audeniq.test>");
    }
    for bad in ["ses", "pigeon", "test,test"] {
        unsafe {
            std::env::set_var("EMAIL_PROVIDERS", bad);
            std::env::remove_var("EMAIL_SES_REGION");
        }
        assert!(Mailer::from_env(ORIGIN).is_err(), "{bad}");
    }
    unsafe {
        std::env::set_var("EMAIL_PROVIDERS", "ses,oci");
        std::env::set_var("EMAIL_SES_REGION", "ap-northeast-2");
        std::env::set_var("EMAIL_SES_USERNAME", "AKIAEXAMPLE");
        std::env::set_var("EMAIL_SES_PASSWORD", "smtp-password");
        std::env::set_var("EMAIL_OCI_REGION", "ap-seoul-1");
        std::env::set_var(
            "EMAIL_OCI_USERNAME",
            "ocid1.user.oc1..example@ocid1.tenancy.oc1..example",
        );
        std::env::set_var("EMAIL_OCI_PASSWORD", "smtp-password");
    }
    let m = Mailer::from_env(ORIGIN).unwrap().unwrap();
    assert_eq!(m.provider_names(), vec!["ses", "oci"]);
    unsafe {
        std::env::set_var("EMAIL_OCI_REGION", "Seoul 1");
    }
    assert!(Mailer::from_env(ORIGIN).is_err());
}
