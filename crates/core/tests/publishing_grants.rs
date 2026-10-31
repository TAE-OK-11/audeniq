//! Publishing through the runtime API role (`audeniq_api`, deploy/grants.sql), as production runs it.
//! The other publishing tests run as the database owner, so a table missing from grants.sql passed them
//! and failed every request in production ("permission denied" → 서버 장애). This walks the artist's
//! publishing screens and staff approval as the API role.
mod support;

use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use support::*;

const SIG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAAABJRU5ErkJggg==";

async fn api_role_app(pool: &PgPool) -> axum::Router {
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
        api_pool,
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
    router(state)
}

fn ok(s: StatusCode, v: &serde_json::Value, what: &str) {
    assert_eq!(s, StatusCode::OK, "{what} as audeniq_api: {v}");
}

#[sqlx::test]
async fn publishing_screens_work_under_the_api_role(pool: PgPool) {
    let app = api_role_app(&pool).await;
    // The signature passed the image check before (owner side, as the other tests do).
    sqlx::query("INSERT INTO catalog.inline_file_safety(source_sha256, rule_version) VALUES($1,'1') ON CONFLICT DO NOTHING")
        .bind(audeniq_core::domain::sha256_hex(SIG))
        .execute(&pool)
        .await
        .unwrap();
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let me = json!({"id": "l1", "creatorId": u.party.to_string(), "name": "서린", "roles": ["composer"], "society": "KOMCA", "ipi": "00712345678", "share": 60.0, "publisherShare": 60.0, "isMe": true});
    let co = json!({"id": "l2", "creatorId": null, "name": "김하늘", "roles": ["lyricist"], "share": 40.0, "publisherShare": 0, "publisher": "뮤직 A"});

    for path in [
        "/publishing/policy",
        "/publishing/summary",
        "/works",
        "/publishing/applications",
        "/publishing/contracts",
        "/publishing/royalties",
        "/ar/submissions",
        "/creators?q=",
    ] {
        let (s, v) = call(&app, "GET", &format!("{base}{path}"), json!({}), Some(&u)).await;
        ok(s, &v, path);
    }
    let (s, v) = call(
        &app,
        "PUT",
        "/api/me/creator",
        json!({"roles": ["composer"], "society": "KOMCA", "ipi": "00712345678"}),
        Some(&u),
    )
    .await;
    ok(s, &v, "creator profile");

    // A released song (another artist's) with the details societies need; its co-writer confirms from a link.
    let (s, w) = call(&app, "POST", &format!("{base}/works"), json!({
        "title": "겨울 편지", "origin": "other_release", "externalRelease": {"artist": "이온", "title": "겨울 편지"},
        "samples": [{"kind": "interpolation", "title": "Old", "artist": "Band"}], "contributors": [me, co]
    }), Some(&u)).await;
    ok(s, &w, "create work");
    let id = w["id"].as_str().unwrap().to_string();
    let (s, v) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    ok(s, &v, "work");
    let (s, v) = call(
        &app,
        "PATCH",
        &format!("{base}/works/{id}"),
        json!({"genre": "Ballad", "row_version": v["rowVersion"]}),
        Some(&u),
    )
    .await;
    ok(s, &v, "edit work");
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/attachments"),
        json!({"kind": "lyrics", "name": "가사", "text": "겨울 편지"}),
        Some(&u),
    )
    .await;
    ok(s, &v, "attachment");
    let cid = v["contributors"][1]["id"].as_str().unwrap().to_string();
    let (s, inv) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/rights/{cid}/invite"),
        json!({"email": "cowriter@example.com"}),
        Some(&u),
    )
    .await;
    ok(s, &inv, "invite");
    let token = inv["token"].as_str().unwrap();
    let (s, v) = call(
        &app,
        "GET",
        &format!("/api/pub-invites/{token}"),
        json!({}),
        None,
    )
    .await;
    ok(s, &v, "invite link");
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/pub-invites/{token}/answer"),
        json!({"agree": true}),
        None,
    )
    .await;
    ok(s, &v, "invite answer");

    // Evidence of authorship (0176) as the API role writes it, and the signer's verified identity.
    let (s, v) = call(&app, "POST", &format!("{base}/works/{id}/attachments"),
        json!({"kind": "project", "name": "겨울 편지 세션 메모", "text": "Logic 프로젝트 2026-03-02"}), Some(&u)).await;
    ok(s, &v, "attach project");
    sqlx::query(
        "INSERT INTO portal.account_identities(user_id,real_name,birth_date,provider,method,transaction_id,ci_hash)
         VALUES($1,'서린','1990-01-01','test','PASS','tx-grants',repeat('b',64))",
    )
    .bind(u.user)
    .execute(&pool)
    .await
    .unwrap();
    // Application, staff approval (makes the contract), signing.
    let (s, a) = call(&app, "POST", &format!("{base}/publishing/applications"), json!({
        "workIds": [id], "territories": ["KR"], "rights": ["performance", "youtube"], "policyVersion": "2026-11",
        "agreed": ["accurate", "rights", "prior", "share"], "signature": {"name": "서린", "method": "drawn", "data": SIG},
        "idempotencyKey": "key-grants-apply",
    }), Some(&u)).await;
    ok(s, &a, "application");
    let staff = user(&app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')")
        .bind(staff.user)
        .execute(&pool)
        .await
        .unwrap();
    let app_id = a["id"].as_str().unwrap();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/applications/{app_id}/claim"),
        json!({}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff claim");
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/applications/{app_id}/decision"),
        json!({"action": "APPROVE", "reason": ""}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff approve");
    let (s, list) = call(
        &app,
        "GET",
        &format!("{base}/publishing/contracts"),
        json!({}),
        Some(&u),
    )
    .await;
    ok(s, &list, "contracts");
    let contract = &list["items"][0];
    let ticks: Vec<String> = contract["confirmations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    let (s, v) = call(&app, "POST", &format!("{base}/publishing/contracts/{}/sign", contract["id"].as_str().unwrap()),
        json!({"signerName": "서린", "signature": SIG, "confirmations": ticks, "rowVersion": contract["rowVersion"]}), Some(&u)).await;
    ok(s, &v, "sign contract");
    let (s, v) = call(
        &app,
        "POST",
        &format!(
            "{base}/publishing/contracts/{}/renewal",
            contract["id"].as_str().unwrap()
        ),
        json!({"months": 12}),
        Some(&u),
    )
    .await;
    ok(s, &v, "renewal request");
    let cid = contract["id"].as_str().unwrap();
    let (s, v) = call(
        &app,
        "GET",
        "/api/staff/publishing/contract-requests",
        json!({}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff contract requests");
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/contracts/{cid}/request"),
        json!({"kind": "renewal", "approve": true}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff renew");
    // A share change of the contracted work, applied by staff.
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    let mut split = w["contributors"].clone();
    split[0]["share"] = json!(50.0);
    split[0]["publisherShare"] = json!(50.0);
    split[1]["share"] = json!(50.0);
    let (s, v) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": split, "row_version": w["rowVersion"]}),
        Some(&u),
    )
    .await;
    ok(s, &v, "share change request");
    let (s, v) = call(
        &app,
        "GET",
        "/api/staff/publishing/changes",
        json!({}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff changes");
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/works/{id}/change"),
        json!({"approve": true}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff apply change");
    // Royalties from a statement, seen by the artist.
    let code = v["internalCode"].as_str().unwrap();
    let (s, v) = call(&app, "POST", "/api/staff/publishing/royalties", json!({"source": "KOMCA", "period": "2026-09", "status": "estimated", "rows": [{"work": code, "kind": "digital", "country": "KR", "collected": 50000}]}), Some(&staff)).await;
    ok(s, &v, "royalty import");
    let (s, v) = call(
        &app,
        "POST",
        &format!(
            "/api/staff/publishing/royalties/{}/status",
            v["id"].as_str().unwrap()
        ),
        json!({"status": "confirmed"}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "royalty confirmed");
    for path in [
        "/api/staff/publishing/royalties".to_string(),
        format!("{base}/publishing/royalties"),
        format!("{base}/works/{id}/royalties"),
        format!("{base}/publishing/summary"),
    ] {
        let who = if path.starts_with("/api/staff") {
            &staff
        } else {
            &u
        };
        let (s, v) = call(&app, "GET", &path, json!({}), Some(who)).await;
        ok(s, &v, &path);
    }

    // Open A&R for a song not released yet, with a demo.
    let (s, demo) = call(&app, "POST", &format!("{base}/works"), json!({"title": "새벽 네 시", "contributors": [{"id": "l1", "creatorId": u.party.to_string(), "name": "서린", "roles": ["composer"], "share": 100.0, "publisherShare": 0, "isMe": true}]}), Some(&u)).await;
    ok(s, &demo, "unreleased work");
    let did = demo["id"].as_str().unwrap();
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/works/{did}/attachments"),
        json!({"kind": "demo", "name": "demo.mp3", "size": 10}),
        Some(&u),
    )
    .await;
    ok(s, &v, "demo");
    let (s, ar) = call(&app, "POST", &format!("{base}/ar/submissions"), json!({"workId": did, "genre": "R&B", "mood": ["잔잔한"], "scope": "internal", "consent": true, "idempotencyKey": "key-grants-ar"}), Some(&u)).await;
    ok(s, &ar, "A&R submit");
    let arid = ar["id"].as_str().unwrap();
    let (s, v) = call(
        &app,
        "GET",
        "/api/staff/publishing/ar",
        json!({}),
        Some(&staff),
    )
    .await;
    ok(s, &v, "staff A&R list");
    for (action, reason) in [("CLAIM", ""), ("FIX", "후렴을 넣어 주세요.")] {
        let (s, v) = call(
            &app,
            "POST",
            &format!("/api/staff/publishing/ar/{arid}/decision"),
            json!({"action": action, "reason": reason}),
            Some(&staff),
        )
        .await;
        ok(s, &v, action);
    }
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions/{arid}/resubmit"),
        json!({"reply": "넣었어요."}),
        Some(&u),
    )
    .await;
    ok(s, &v, "A&R resubmit");
    let (s, v) = call(
        &app,
        "POST",
        &format!(
            "{base}/ar/submissions/{}/withdraw",
            ar["id"].as_str().unwrap()
        ),
        json!({}),
        Some(&u),
    )
    .await;
    ok(s, &v, "A&R withdraw");

    // Album prepare from a release's credits.
    let release = create_release(&app, &u).await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/releases/{release}/works"),
        json!({"enroll": false}),
        Some(&u),
    )
    .await;
    ok(s, &v, "prepare from release");
}
