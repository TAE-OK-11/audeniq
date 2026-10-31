//! Publishing stage 1 against real PostgreSQL: works, shares, recordings,
//! attachments, the administration application (접수 · 철회 · 보완 재제출)
//! and staff review, in the shapes the Studio screens read.
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod support;
/// A real 1×1 PNG: the signature goes through the same check as 배급 신청서.
const SIG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
use support::{User, app, call, create_artist, create_release, row_version, user};

async fn staff_user(app: &Router, pool: &PgPool) -> User {
    let s = user(app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'REVIEWER','test-operator')")
        .bind(s.user)
        .execute(pool)
        .await
        .unwrap();
    s
}

/// The signature passed the image check before (as in mcp.rs / stage1.rs):
/// the media engine that re-encodes it is not needed for these flows.
async fn admit_signature(pool: &PgPool) {
    sqlx::query("INSERT INTO catalog.inline_file_safety(source_sha256, rule_version) VALUES($1,'1') ON CONFLICT DO NOTHING")
        .bind(audeniq_core::domain::sha256_hex(SIG))
        .execute(pool)
        .await
        .unwrap();
}

async fn track(app: &Router, pool: &PgPool, u: &User, release: Uuid, title: &str, no: i32) -> Uuid {
    let artist = create_artist(app, u).await;
    let rv = row_version(pool, release).await;
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
        json!({"title": title, "disc_number": 1, "track_number": no, "artist_id": artist, "row_version": rv}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

fn people(me: &User, my_share: f64, other_share: f64) -> Value {
    json!([
        {"id": "local-1", "creatorId": me.party.to_string(), "name": "서린", "roles": ["lyricist", "composer"], "society": "KOMCA", "ipi": "00712345678", "share": my_share, "publisherShare": my_share, "isMe": true},
        {"id": "local-2", "creatorId": null, "name": "김하늘", "roles": ["composer"], "society": "KOMCA", "ipi": null, "share": other_share, "publisherShare": 0}
    ])
}

fn apply(works: &[&str], key: &str) -> Value {
    json!({
        "workIds": works, "territories": ["KR", "WORLD"], "rights": ["performance", "mechanical", "digital"],
        "policyVersion": "2026-10", "agreed": ["terms", "rights", "exclusive", "share"],
        "signature": {"name": "서린", "method": "drawn", "data": SIG},
        "idempotencyKey": key,
    })
}

#[sqlx::test]
async fn work_to_application_to_review(pool: PgPool) {
    let (app, _store) = app(pool.clone()).await;
    admit_signature(&pool).await;
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let release = create_release(&app, &u).await;
    let t1 = track(&app, &pool, &u, release, "파도", 1).await;

    // Policy the application screen reads.
    let (s, p) = call(
        &app,
        "GET",
        &format!("{base}/publishing/policy"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, p["version"].as_str()),
        (StatusCode::OK, Some("2026-10"))
    );

    // A work from a released track, shares still being split (90%).
    let (s, w) = call(&app, "POST", &format!("{base}/works"), json!({
        "title": "파도", "altTitles": [], "workType": "original", "language": "ko", "genre": "Indie Pop", "description": "",
        "visibility": "private", "iswc": null, "hasPriorPublisher": false, "priorPublisher": null,
        "contributors": people(&u, 50.0, 40.0), "attachments": [],
        "recording": {"releaseId": release, "trackId": t1, "title": "파도", "releaseTitle": "Draft"}
    }), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{w}");
    let id = w["id"].as_str().unwrap().to_string();
    assert!(w["internalCode"].as_str().unwrap().starts_with("AQW-"));
    assert_eq!(w["registration"], "none");
    assert_eq!(w["myRole"], "owner");
    assert_eq!(w["recordings"][0]["trackId"], t1.to_string());
    // The linked recording reads like the release list (Studio chip), not the raw pipeline stage.
    assert_eq!(w["recordings"][0]["status"], "draft");
    assert_eq!(w["contributors"][0]["isMe"], true);
    assert_eq!(w["contributors"][0]["confirm"], "confirmed");
    assert_eq!(w["contributors"][1]["confirm"], "pending");
    assert_eq!(w["contributors"][1]["share"], 40.0);

    // Found by track, listed.
    let (_, by_track) = call(
        &app,
        "GET",
        &format!("{base}/works?track_id={t1}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(by_track["items"].as_array().unwrap().len(), 1);

    // 90% cannot be submitted.
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&id], "key-under-100"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert_eq!(e["error"]["code"], "WORK_SPLIT_INCOMPLETE");

    // A stale row version is refused; the current one saves 60 : 40.
    let (s, _) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 60.0, 40.0), "row_version": 0}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let rv = w["rowVersion"].as_i64().unwrap();
    let mut keep = people(&u, 60.0, 40.0);
    keep[1]["id"] = w["contributors"][1]["id"].clone();
    let (s, w) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": keep, "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["contributors"][0]["share"], 60.0);

    // Over 100% is refused before anything is stored.
    let rv = w["rowVersion"].as_i64().unwrap();
    let (s, e) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 70.0, 40.0), "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("WORK_SHARE_TOTAL_OVER_100"))
    );

    // Lyrics attached as text show in place.
    let (s, w) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/attachments"),
        json!({"kind": "lyrics", "name": "파도.txt", "size": 20, "text": "밀려오는 파도처럼"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["attachments"][0]["text"], "밀려오는 파도처럼");

    // Submit (신청서 접수); the same key returns the same application.
    let (s, a) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&id], "key-first-apply"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["status"], "submitted");
    assert!(a["number"].as_str().unwrap().starts_with("AUD-"));
    // The signed original is a 배급 신청서 record (form PUBLISHING), sealed.
    let (form, sealed): (String, bool) = sqlx::query_as(
        "SELECT ra.form, EXISTS(SELECT 1 FROM operations.document_seals s WHERE s.doc_kind='APPLICATION' AND s.doc_id=ra.id)
           FROM portal.release_applications ra WHERE ra.application_no=$1 AND ra.release_id IS NULL",
    )
    .bind(a["number"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((form.as_str(), sealed), ("PUBLISHING", true));
    let app_id = a["id"].as_str().unwrap().to_string();
    let (_, again) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&id], "key-first-apply"),
        Some(&u),
    )
    .await;
    assert_eq!(again["id"], a["id"]);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM portal.pub_applications WHERE org_id=$1")
        .bind(u.org)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // ...and a second application for the same work is refused.
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&id], "key-second-apply"),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("WORK_ALREADY_APPLIED")
        )
    );
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(w["registration"], "internal_review");
    assert_eq!(w["application"]["status"], "submitted");
    // Shares are locked while under review.
    let rv = w["rowVersion"].as_i64().unwrap();
    let (s, e) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 50.0, 50.0), "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("WORK_UNDER_REVIEW"))
    );

    // Staff: only staff see the queue; a reviewer asks for a fix.
    let (s, _) = call(
        &app,
        "GET",
        "/api/staff/publishing/applications",
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let st = staff_user(&app, &pool).await;
    let (s, q) = call(
        &app,
        "GET",
        "/api/staff/publishing/applications?status=submitted",
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{q}");
    assert!(
        q["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == a["id"])
    );
    let (s, one) = call(
        &app,
        "GET",
        &format!("/api/staff/publishing/applications/{app_id}"),
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{one}");
    assert_eq!(one["works"][0]["contributors"].as_array().unwrap().len(), 2);
    let decision_path = format!("/api/staff/publishing/applications/{app_id}/decision");
    let decide = |d: Value| call(&app, "POST", &decision_path, d, Some(&st));
    let (s, x) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/applications/{app_id}/claim"),
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(
        (s, x["status"].as_str()),
        (StatusCode::OK, Some("in_review"))
    );
    let (s, e) = decide(json!({"action": "REQUEST_CORRECTION"})).await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("APPLICATION_REASON_REQUIRED"))
    );
    let (s, e) = decide(json!({"action": "REQUEST_CORRECTION", "fields": ["work.title"]})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    let (s, x) = decide(json!({"action": "REQUEST_CORRECTION", "reason": "김하늘 님 IPI를 적어 주세요.", "fields": ["contributors.ipi"], "documents": ["공동 작곡 합의서"]})).await;
    assert_eq!(s, StatusCode::OK, "{x}");
    assert_eq!(x["status"], "needs_fix");
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(w["registration"], "needs_fix");
    assert_eq!(w["fix"]["documents"][0], "공동 작곡 합의서");
    // What to fix is the summary; the reviewer's own words are the note Studio shows as 검토팀 의견.
    assert_eq!(
        w["fix"]["reason"],
        "참여자 IPI 번호 · 서류 공동 작곡 합의서 보완"
    );
    assert_eq!(w["fix"]["note"], "김하늘 님 IPI를 적어 주세요.");
    // The document waits on 권리·보완 서류 like a release proof, with its notice.
    let (_, docs) = call(
        &app,
        "GET",
        &format!("{base}/documents"),
        json!({}),
        Some(&u),
    )
    .await;
    let doc = docs["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["title"] == "파도 · 공동 작곡 합의서")
        .cloned()
        .expect("requested document");
    let (_, notes) = call(
        &app,
        "GET",
        &format!("{base}/notifications"),
        json!({}),
        Some(&u),
    )
    .await;
    let text = notes.to_string();
    assert!(
        text.contains("저작권 보완 요청") && text.contains("제출할 서류가 있어요"),
        "{text}"
    );

    // The writer fixes the shares (allowed while asked) and resubmits.
    let rv = w["rowVersion"].as_i64().unwrap();
    let mut fixed = people(&u, 60.0, 40.0);
    fixed[1]["ipi"] = json!("00755511122");
    let (s, w) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": fixed, "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    // Not before the document is in.
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/registration/resubmit"),
        json!({"note": "IPI를 적었어요."}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("WORK_DOCUMENTS_DUE"))
    );
    sqlx::query(
        "UPDATE portal.documents SET status='REVIEW', file_name='합의서.pdf' WHERE id=$1::uuid",
    )
    .bind(doc["id"].as_str().unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let (s, w) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/registration/resubmit"),
        json!({"note": "IPI를 적었어요.", "files": ["합의서.pdf"]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["registration"], "internal_review");
    assert!(w["fix"].is_null());
    let (_, apps) = call(
        &app,
        "GET",
        &format!("{base}/publishing/applications"),
        json!({}),
        Some(&u),
    )
    .await;
    let mine = &apps["items"][0];
    assert_eq!(mine["status"], "in_review");
    assert_eq!(mine["replyNote"], "IPI를 적었어요.");
    // History is the audit trail of the application.
    let steps: Vec<&str> = mine["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["text"].as_str())
        .collect();
    assert_eq!(steps, ["제출 완료", "심사 시작", "보완 요청", "보완 제출"]);

    // Approve, then register with the society.
    let (s, x) = decide(json!({"action": "APPROVE"})).await;
    assert_eq!(
        (s, x["status"].as_str()),
        (StatusCode::OK, Some("approved"))
    );
    let (s, _) = decide(json!({"action": "REJECT", "reason": "늦은 반려"})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = decide(json!({"action": "REGISTER", "society": "KOMCA"})).await;
    assert_eq!(s, StatusCode::OK);
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(w["registration"], "registered");
    assert_eq!(
        w["registrationLog"].as_array().unwrap().last().unwrap()["society"],
        "KOMCA"
    );

    // A registered work keeps its shares in force; the change waits for review.
    let rv = w["rowVersion"].as_i64().unwrap();
    let (s, w) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 50.0, 50.0), "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["contributors"][0]["share"], 60.0);
    assert_eq!(w["pendingChange"]["contributors"][0]["share"], 50.0);

    // A registered work cannot be deleted; everything was audited.
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE org_id=$1 AND action LIKE 'publishing.%'")
        .bind(u.org)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(audited >= 10, "{audited}");

    // Summary counts it as managed.
    let (_, sum) = call(
        &app,
        "GET",
        &format!("{base}/publishing/summary"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(sum["works"]["managed"], 1);
    assert_eq!(sum["enrolled"], true);
}

#[sqlx::test]
async fn prepare_from_credits_withdraw_and_isolation(pool: PgPool) {
    let (app, _store) = app(pool.clone()).await;
    admit_signature(&pool).await;
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let release = create_release(&app, &u).await;
    let t1 = track(&app, &pool, &u, release, "한낮", 1).await;
    let t2 = track(&app, &pool, &u, release, "밤바다", 2).await;
    // Credits: the writer composed both, a co-writer wrote the lyrics of the first.
    let co = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON','박도윤')",
    )
    .bind(co)
    .bind(u.org)
    .execute(&pool)
    .await
    .unwrap();
    for (t, p, r) in [
        (t1, u.party, "COMPOSER"),
        (t1, co, "LYRICIST"),
        (t2, u.party, "COMPOSER"),
    ] {
        sqlx::query(
            "INSERT INTO catalog.credits(track_id,org_id,party_id,role) VALUES($1,$2,$3,$4)",
        )
        .bind(t)
        .bind(u.org)
        .bind(p)
        .bind(r)
        .execute(&pool)
        .await
        .unwrap();
    }
    let (s, r) = call(
        &app,
        "POST",
        &format!("{base}/releases/{release}/works"),
        json!({"enroll": true, "skip_titles": []}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["created"], 2);
    // Running again makes nothing new.
    let (_, r) = call(
        &app,
        "POST",
        &format!("{base}/releases/{release}/works"),
        json!({"enroll": true}),
        Some(&u),
    )
    .await;
    assert_eq!(r["created"], 0);
    let (_, list) = call(&app, "GET", &format!("{base}/works"), json!({}), Some(&u)).await;
    let items = list["items"].as_array().unwrap();
    let first = items.iter().find(|w| w["title"] == "한낮").unwrap();
    assert_eq!(first["registration"], "preparing");
    let shares: Vec<f64> = first["contributors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["share"].as_f64().unwrap())
        .collect();
    assert_eq!(shares.iter().sum::<f64>(), 100.0);
    let second = items.iter().find(|w| w["title"] == "밤바다").unwrap();
    assert_eq!(second["contributors"][0]["isMe"], true);

    // Apply both, then withdraw: the works return to 'none'.
    let ids: Vec<&str> = items.iter().map(|w| w["id"].as_str().unwrap()).collect();
    let (s, a) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&ids, "key-two-works"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["workIds"].as_array().unwrap().len(), 2);
    let (s, a) = call(
        &app,
        "POST",
        &format!(
            "{base}/publishing/applications/{}/withdraw",
            a["id"].as_str().unwrap()
        ),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, a["status"].as_str()),
        (StatusCode::OK, Some("withdrawn"))
    );
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{}", ids[0]),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(w["registration"], "none");

    // Policy and agreement checks.
    let mut bad = apply(&ids, "key-old-policy");
    bad["policyVersion"] = json!("2025-01");
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        bad,
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("POLICY_CHANGED"))
    );
    let mut bad = apply(&ids, "key-no-agree");
    bad["agreed"] = json!(["terms"]);
    let (s, _) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        bad,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Another org sees nothing and changes nothing.
    let other = user(&app).await;
    let (s, _) = call(
        &app,
        "GET",
        &format!("{base}/works"),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/works/{}", other.org, ids[0]),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/publishing/applications", other.org),
        apply(&ids, "key-foreign"),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Archive an unapplied work; it leaves the list.
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("{base}/works/{}", ids[1]),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = call(&app, "GET", &format!("{base}/works"), json!({}), Some(&u)).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // The writer's own IPI and society.
    let (s, me) = call(
        &app,
        "PUT",
        "/api/me/creator",
        json!({"roles": ["composer"], "society": "KOMCA", "ipi": "00712345678"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert_eq!(me["ipi"], "00712345678");
    let (_, found) = call(
        &app,
        "GET",
        &format!("{base}/creators?q=박"),
        json!({}),
        Some(&u),
    )
    .await;
    assert!(
        found["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "박도윤")
    );
    // Stage 2 lists answer empty.
    let (s, c) = call(
        &app,
        "GET",
        &format!("{base}/publishing/contracts"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, c["items"].as_array().map(Vec::len)),
        (StatusCode::OK, Some(0))
    );
}
