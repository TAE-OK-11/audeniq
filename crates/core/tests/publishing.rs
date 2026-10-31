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
        "policyVersion": "2026-11", "agreed": ["accurate", "rights", "prior", "share"],
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
        (StatusCode::OK, Some("2026-11"))
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

    // Approval made the contract (계약서) with the confirmed terms; it waits for the artist's signature,
    // and the society registration waits for the contract.
    let (s, list) = call(
        &app,
        "GET",
        &format!("{base}/publishing/contracts"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let contract = list["items"][0].clone();
    assert_eq!(contract["status"], "pending_sign");
    assert_eq!(contract["commissionRate"], 10.0);
    assert!(contract["number"].as_str().unwrap().starts_with("AQP-"));
    assert!(contract["body"].as_str().unwrap().contains("‘파도’"));
    assert_eq!(contract["works"][0]["title"], "파도");
    assert_eq!(contract["releaseIds"][0], release.to_string());
    let cid = contract["id"].as_str().unwrap().to_string();
    let (s, e) = decide(json!({"action": "REGISTER", "society": "KOMCA"})).await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("CONTRACT_NOT_SIGNED")
        ),
        "{e}"
    );
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert!(
        w["contractId"].is_null(),
        "an unsigned contract does not count yet"
    );
    let sign = |ticks: Vec<&str>| json!({"signerName": "서린", "signature": SIG, "confirmations": ticks, "rowVersion": contract["rowVersion"]});
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/publishing/contracts/{cid}/sign"),
        sign(vec!["read"]),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("AGREEMENT_CONFIRMATION_REQUIRED")
        ),
        "{e}"
    );
    let (s, c) = call(
        &app,
        "POST",
        &format!("{base}/publishing/contracts/{cid}/sign"),
        sign(vec!["read", "rights", "exclusive"]),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    assert_eq!(c["status"], "active");
    assert_eq!(c["signerName"], "서린");
    assert!(c["contentHash"].as_str().is_some_and(|h| h.len() == 64));
    assert!(
        !c["startDate"].as_str().unwrap().is_empty() && !c["endDate"].as_str().unwrap().is_empty()
    );
    let (s, _) = call(
        &app,
        "POST",
        &format!("{base}/publishing/contracts/{cid}/sign"),
        sign(vec!["read", "rights", "exclusive"]),
        Some(&u),
    )
    .await;
    assert_ne!(s, StatusCode::OK, "a contract is signed once");
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(w["contractId"], cid);
    // Signed but not registered yet: the contract is in force, so a share change waits for review too.
    let rv = w["rowVersion"].as_i64().unwrap();
    let (s, signed) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 55.0, 45.0), "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{signed}");
    assert_eq!(signed["contributors"][0]["share"], 60.0);
    assert_eq!(signed["pendingChange"]["contributors"][0]["share"], 55.0);

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
    // What the release form already holds: genre, language, lyrics, and a second track marked instrumental.
    sqlx::query("UPDATE catalog.releases SET draft = draft || jsonb_build_object('genre','R&B','language','en','draftTracks',jsonb_build_array(jsonb_build_object('serverId',$2::text,'instrumental',true))), row_version=row_version+1 WHERE id=$1")
        .bind(release)
        .bind(t2)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.tracks SET lyrics=$2 WHERE id=$1")
        .bind(t1)
        .bind("한낮의 햇살 아래\n걸어가")
        .execute(&pool)
        .await
        .unwrap();
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
    // Nothing typed twice: genre and language from the release, lyrics as a lyrics file, instrumental kept.
    assert_eq!(
        (first["genre"].as_str(), first["language"].as_str()),
        (Some("R&B"), Some("en"))
    );
    assert_eq!(first["workType"], "original");
    assert_eq!(first["attachments"][0]["kind"], "lyrics");
    assert_eq!(first["attachments"][0]["text"], "한낮의 햇살 아래\n걸어가");
    assert_eq!(second["workType"], "instrumental");
    assert!(second["attachments"].as_array().unwrap().is_empty());

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
    bad["agreed"] = json!(["accurate"]);
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

    // Archive an unapplied work (by title: both were made in one transaction, so list order is by id); it leaves the list.
    let night = items.iter().find(|w| w["title"] == "밤바다").unwrap()["id"]
        .as_str()
        .unwrap();
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("{base}/works/{night}"),
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

/// Publishing takes a song three ways (migration 0169): not released yet (Open A&R), released by the
/// artist (read from the catalog), released by someone else (the artist gives the release).
#[sqlx::test]
async fn work_origin_decides_what_can_be_applied(pool: PgPool) {
    let (app, _store) = app(pool.clone()).await;
    admit_signature(&pool).await;
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let new_work = |title: &str, extra: Value| {
        let mut body = json!({"title": title, "contributors": people(&u, 60.0, 40.0)});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        body
    };

    // Not released yet: kept, but administration waits for a release.
    let (s, demo) = call(
        &app,
        "POST",
        &format!("{base}/works"),
        new_work("새벽 네 시", json!({})),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{demo}");
    assert_eq!(demo["origin"], "unreleased");
    assert!(demo["externalRelease"].is_null());
    let demo_id = demo["id"].as_str().unwrap().to_string();
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&demo_id], "key-unreleased"),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("WORK_NOT_RELEASED"))
    );

    // Released by someone else: the release is needed and kept as given.
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/works"),
        new_work("겨울 편지", json!({"origin": "other_release"})),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("WORK_RELEASE_INFO_REQUIRED"))
    );
    let (s, other) = call(&app, "POST", &format!("{base}/works"), new_work("겨울 편지", json!({
        "origin": "other_release",
        "externalRelease": {"artist": "이온", "title": "겨울 편지", "releaseDate": "2025-11-20", "isrc": "kr-a26-25-00011", "label": null}
    })), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{other}");
    assert_eq!(other["origin"], "other_release");
    assert_eq!(other["externalRelease"]["artist"], "이온");
    assert_eq!(other["externalRelease"]["isrc"], "KRA262500011");
    let other_id = other["id"].as_str().unwrap().to_string();
    let (s, a) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&other_id], "key-other-release"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");

    // A song made entirely by AI is kept but cannot be applied.
    let (s, ai) = call(&app, "POST", &format!("{base}/works"), new_work("인공 노래", json!({
        "origin": "other_release", "aiUse": "generated", "externalRelease": {"artist": "하람", "title": "인공 노래"}
    })), Some(&u)).await;
    assert_eq!(
        (s, ai["aiUse"].as_str()),
        (StatusCode::OK, Some("generated")),
        "{ai}"
    );
    let ai_id = ai["id"].as_str().unwrap().to_string();
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&ai_id], "key-ai-generated"),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("WORK_NOT_ELIGIBLE"))
    );

    // The pitched song was released by another artist: it moves to other_release with that release.
    let rv = demo["rowVersion"].as_i64().unwrap();
    let (s, e) = call(
        &app,
        "PATCH",
        &format!("{base}/works/{demo_id}"),
        json!({"origin": "own_release", "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("WORK_ORIGIN_INVALID"))
    );
    let (s, e) = call(
        &app,
        "PATCH",
        &format!("{base}/works/{demo_id}"),
        json!({"origin": "other_release", "row_version": rv}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("WORK_RELEASE_INFO_REQUIRED"))
    );

    // Released by the artist: linking a recording of the org's own release makes it own_release.
    let release = create_release(&app, &u).await;
    let t = track(&app, &pool, &u, release, "새벽 네 시", 1).await;
    let (s, w) = call(
        &app,
        "POST",
        &format!("{base}/works/{demo_id}/recordings"),
        json!({"release_id": release, "track_id": t}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["origin"], "own_release");
    let (s, a) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&demo_id], "key-own-release"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
}

/// A co-writer confirms their share from a link (migration 0171): copied by the writer and/or mailed,
/// no account needed, one answer per link.
#[sqlx::test]
async fn co_writer_confirms_from_a_link(pool: PgPool) {
    let (app, _store) = app(pool.clone()).await;
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let (s, w) = call(
        &app,
        "POST",
        &format!("{base}/works"),
        json!({"title": "파도", "contributors": people(&u, 60.0, 40.0)}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    let id = w["id"].as_str().unwrap().to_string();
    let me = w["contributors"][0]["id"].as_str().unwrap().to_string();
    let co = w["contributors"][1]["id"].as_str().unwrap().to_string();

    // Not the writer's own row; a bad address is refused.
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/rights/{me}/invite"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("WORK_INVITE_SELF"))
    );
    let (s, _) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/rights/{co}/invite"),
        json!({"email": "nope"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A copied link, then a mailed one: asking again replaces the link.
    let (s, first) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/rights/{co}/invite"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, first["emailed"].as_bool()),
        (StatusCode::OK, Some(false)),
        "{first}"
    );
    let old = first["token"].as_str().unwrap().to_string();
    let co_email = format!("cowriter-{}@example.com", Uuid::new_v4().simple());
    let (_, second) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/rights/{co}/invite"),
        json!({"email": co_email}),
        Some(&u),
    )
    .await;
    assert_eq!(second["emailed"], true);
    let token = second["token"].as_str().unwrap().to_string();
    let (s, _) = call(
        &app,
        "GET",
        &format!("/api/pub-invites/{old}"),
        json!({}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "the first link stopped working");
    let plain: bool = sqlx::query_scalar("SELECT bool_or(position('example.com' in encode(email_cipher,'escape'))>0) FROM portal.pub_split_invites")
        .fetch_one(&pool).await.unwrap();
    assert!(!plain, "the address is sealed");

    // The mail carries the same link.
    unsafe {
        std::env::set_var("EMAIL_FROM", "AUDENIQ <no-reply@audeniq.test>");
        std::env::set_var("EMAIL_PROVIDERS", "test");
    }
    let mailer = audeniq_core::email::Mailer::from_env("https://studio.audeniq.test")
        .unwrap()
        .unwrap();
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    assert_eq!(
        audeniq_core::publishing::dispatch_invites(&pool, &keys, &mailer)
            .await
            .unwrap(),
        1
    );
    let sent = audeniq_core::email::test_sent_to(&co_email);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].text.contains(&format!("/split-confirm/{token}")),
        "{}",
        sent[0].text
    );

    // The co-writer sees the split without an account, disputes once with a note.
    let (s, inv) = call(
        &app,
        "GET",
        &format!("/api/pub-invites/{token}"),
        json!({}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{inv}");
    assert_eq!(inv["title"], "파도");
    assert_eq!(inv["people"][1]["you"], true);
    let (s, a) = call(
        &app,
        "POST",
        &format!("/api/pub-invites/{token}/answer"),
        json!({"agree": false, "note": "제 몫은 50%예요."}),
        None,
    )
    .await;
    assert_eq!(
        (s, a["answer"].as_str()),
        (StatusCode::OK, Some("disputed")),
        "{a}"
    );
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/pub-invites/{token}/answer"),
        json!({"agree": true}),
        None,
    )
    .await;
    assert_ne!(s, StatusCode::OK, "one answer per link");
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(w["contributors"][1]["confirm"], "disputed");
    assert_eq!(w["contributors"][1]["inviteNote"], "제 몫은 50%예요.");
    let erased: bool = sqlx::query_scalar("SELECT bool_and(email_cipher IS NULL AND token_cipher IS NULL) FROM portal.pub_split_invites")
        .fetch_one(&pool).await.unwrap();
    assert!(erased, "address and link are erased with the answer");
}

/// Open A&R on the server (0172), and the work details societies need.
#[sqlx::test]
async fn open_ar_and_work_details(pool: PgPool) {
    let (app, _store) = app(pool.clone()).await;
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let mut co = people(&u, 60.0, 40.0);
    co[1]["publisher"] = json!("뮤직 퍼블리싱 A");
    let (s, w) = call(&app, "POST", &format!("{base}/works"), json!({
        "title": "새벽 네 시 (Remix)", "workType": "arrangement", "contributors": co,
        "originalWork": {"title": "새벽 네 시", "writers": "서린, 박도윤", "publisher": null, "permission": true},
        "samples": [{"kind": "sample", "title": "Old Song", "artist": "Old Band", "owners": "Old Records"}],
        "priorPublisherUntil": "2026-12-31"
    }), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["contributors"][1]["publisher"], "뮤직 퍼블리싱 A");
    assert_eq!(w["originalWork"]["title"], "새벽 네 시");
    assert_eq!(w["originalWork"]["permission"], true);
    assert_eq!(w["samples"][0]["kind"], "sample");
    assert_eq!(w["priorPublisherUntil"], "2026-12-31");
    let id = w["id"].as_str().unwrap().to_string();
    let (s, e) = call(
        &app,
        "PATCH",
        &format!("{base}/works/{id}"),
        json!({"samples": [{"kind": "cover", "title": "x"}], "row_version": w["rowVersion"]}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("WORK_SAMPLES_INVALID"))
    );

    // A&R: a demo is needed; then one open submission per work; the same key returns the first.
    let ar = |key: &str| json!({"workId": id, "genre": "R&B", "mood": ["잔잔한"], "description": "", "references": "", "scope": "partners", "sync": true, "consent": true, "idempotencyKey": key});
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions"),
        ar("key-ar-nodemo"),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("AR_DEMO_REQUIRED"))
    );
    let (s, _) = call(
        &app,
        "POST",
        &format!("{base}/works/{id}/attachments"),
        json!({"kind": "demo", "name": "demo.mp3", "size": 10}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, a) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions"),
        ar("key-ar-first"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(
        (
            a["status"].as_str(),
            a["sync"].as_bool(),
            a["workTitle"].as_str()
        ),
        (Some("submitted"), Some(true), Some("새벽 네 시 (Remix)"))
    );
    assert_eq!(a["history"][0]["text"], "제출 완료");
    let (_, again) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions"),
        ar("key-ar-first"),
        Some(&u),
    )
    .await;
    assert_eq!(again["id"], a["id"]);
    let (s, e) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions"),
        ar("key-ar-second"),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("DUPLICATE_SUBMISSION")
        )
    );
    let (s, list) = call(
        &app,
        "GET",
        &format!("{base}/ar/submissions"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, list["items"].as_array().map(Vec::len)),
        (StatusCode::OK, Some(1))
    );
    let aid = a["id"].as_str().unwrap();
    let (s, closed) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions/{aid}/withdraw"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, closed["status"].as_str()),
        (StatusCode::OK, Some("closed"))
    );
    // A released song is administered, not pitched.
    let (_, rel) = call(&app, "POST", &format!("{base}/works"), json!({"title": "발매곡", "origin": "other_release", "externalRelease": {"artist": "이온", "title": "발매곡"}, "contributors": people(&u, 60.0, 40.0)}), Some(&u)).await;
    let rid = rel["id"].as_str().unwrap();
    let (s, e) = call(&app, "POST", &format!("{base}/ar/submissions"), json!({"workId": rid, "genre": "R&B", "mood": ["잔잔한"], "scope": "internal", "consent": true, "idempotencyKey": "key-ar-released"}), Some(&u)).await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("WORK_RELEASED"))
    );
}

/// Staff answer what artists ask (0173): share changes, contract renewal / termination, Open A&R review, and
/// royalties from a society statement that the artist then sees.
#[sqlx::test]
async fn staff_answer_artist_requests(pool: PgPool) {
    let (app, _store) = app(pool.clone()).await;
    admit_signature(&pool).await;
    let u = user(&app).await;
    let base = format!("/api/orgs/{}", u.org);
    let st = user(&app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')")
        .bind(st.user).execute(&pool).await.unwrap();

    // A managed work: applied, approved, signed, registered.
    let (_, w) = call(&app, "POST", &format!("{base}/works"), json!({
        "title": "겨울 편지", "origin": "other_release", "externalRelease": {"artist": "이온", "title": "겨울 편지"}, "contributors": people(&u, 60.0, 40.0)
    }), Some(&u)).await;
    let id = w["id"].as_str().unwrap().to_string();
    let code = w["internalCode"].as_str().unwrap().to_string();
    let (_, a) = call(
        &app,
        "POST",
        &format!("{base}/publishing/applications"),
        apply(&[&id], "key-staff-ops"),
        Some(&u),
    )
    .await;
    let aid = a["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/staff/publishing/applications/{aid}/claim"),
        json!({}),
        Some(&st),
    )
    .await;
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/applications/{aid}/decision"),
        json!({"action": "APPROVE", "reason": ""}),
        Some(&st),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = call(
        &app,
        "GET",
        &format!("{base}/publishing/contracts"),
        json!({}),
        Some(&u),
    )
    .await;
    let c = list["items"][0].clone();
    let cid = c["id"].as_str().unwrap().to_string();
    let ticks: Vec<String> = c["confirmations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap().to_string())
        .collect();
    let (s, signed) = call(&app, "POST", &format!("{base}/publishing/contracts/{cid}/sign"), json!({"signerName": "서린", "signature": SIG, "confirmations": ticks, "rowVersion": c["rowVersion"]}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{signed}");
    let end = signed["endDate"].as_str().unwrap().to_string();

    // Share change: rejected with a reason, asked again, applied.
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    let (_, w) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 50.0, 50.0), "row_version": w["rowVersion"]}),
        Some(&u),
    )
    .await;
    assert_eq!(w["pendingChange"]["contributors"][0]["share"], 50.0);
    let (_, changes) = call(
        &app,
        "GET",
        "/api/staff/publishing/changes",
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(changes["items"][0]["work"]["id"], id.as_str());
    let (s, e) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/works/{id}/change"),
        json!({"approve": false}),
        Some(&st),
    )
    .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("STAFF_REASON_REQUIRED"))
    );
    call(
        &app,
        "POST",
        &format!("/api/staff/publishing/works/{id}/change"),
        json!({"approve": false, "reason": "공동 작곡 합의서가 필요해요."}),
        Some(&st),
    )
    .await;
    let (_, w) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}"),
        json!({}),
        Some(&u),
    )
    .await;
    assert!(w["pendingChange"].is_null());
    assert_eq!(w["changeDecision"]["outcome"], "rejected");
    assert_eq!(w["changeDecision"]["note"], "공동 작곡 합의서가 필요해요.");
    let (_, w) = call(
        &app,
        "PUT",
        &format!("{base}/works/{id}/rights"),
        json!({"contributors": people(&u, 50.0, 50.0), "row_version": w["rowVersion"]}),
        Some(&u),
    )
    .await;
    assert!(
        w["changeDecision"].is_null(),
        "a new request clears the last answer"
    );
    let (s, w) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/works/{id}/change"),
        json!({"approve": true}),
        Some(&st),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["contributors"][0]["share"], 50.0);
    assert_eq!(
        w["contributors"][1]["confirm"], "pending",
        "a co-writer whose share changed confirms again"
    );

    // Renewal approved: the end date moves 12 months. Termination asked, then rejected.
    call(
        &app,
        "POST",
        &format!("{base}/publishing/contracts/{cid}/renewal"),
        json!({"months": 12}),
        Some(&u),
    )
    .await;
    let (_, reqs) = call(
        &app,
        "GET",
        "/api/staff/publishing/contract-requests",
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(reqs["items"][0]["renewal"]["months"], 12);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/contracts/{cid}/request"),
        json!({"kind": "renewal", "approve": true}),
        Some(&st),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = call(
        &app,
        "GET",
        &format!("{base}/publishing/contracts"),
        json!({}),
        Some(&u),
    )
    .await;
    let renewed = &list["items"][0];
    assert!(renewed["renewal"].is_null());
    assert_eq!(renewed["requestDecision"]["outcome"], "renewed");
    let next_year = format!("{}{}", end[..4].parse::<i32>().unwrap() + 1, &end[4..]);
    assert_eq!(renewed["endDate"], next_year.as_str());
    call(
        &app,
        "POST",
        &format!("{base}/publishing/contracts/{cid}/termination"),
        json!({}),
        Some(&u),
    )
    .await;
    call(&app, "POST", &format!("/api/staff/publishing/contracts/{cid}/request"), json!({"kind": "termination", "approve": false, "reason": "관리 단체 절차가 진행 중이에요."}), Some(&st)).await;
    let (_, list) = call(
        &app,
        "GET",
        &format!("{base}/publishing/contracts"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (
            list["items"][0]["status"].as_str(),
            list["items"][0]["requestDecision"]["outcome"].as_str()
        ),
        (Some("active"), Some("termination_rejected"))
    );

    // Open A&R review: claim, fix, the artist answers, reviewed, proposing, closed.
    let (_, demo) = call(
        &app,
        "POST",
        &format!("{base}/works"),
        json!({"title": "새벽 네 시", "contributors": people(&u, 60.0, 40.0)}),
        Some(&u),
    )
    .await;
    let did = demo["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        &format!("{base}/works/{did}/attachments"),
        json!({"kind": "demo", "name": "demo.mp3", "size": 10}),
        Some(&u),
    )
    .await;
    let (_, ar) = call(&app, "POST", &format!("{base}/ar/submissions"), json!({"workId": did, "genre": "R&B", "mood": ["잔잔한"], "scope": "partners", "consent": true, "idempotencyKey": "key-staff-ar"}), Some(&u)).await;
    let arid = ar["id"].as_str().unwrap();
    let (_, queue) = call(
        &app,
        "GET",
        "/api/staff/publishing/ar?status=submitted",
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(queue["items"][0]["demoName"], "demo.mp3");
    let ar_path = format!("/api/staff/publishing/ar/{arid}/decision");
    let decide = |action: &str, reason: &str| {
        call(
            &app,
            "POST",
            &ar_path,
            json!({"action": action, "reason": reason}),
            Some(&st),
        )
    };
    assert_eq!(decide("CLAIM", "").await.1["status"], "in_review");
    assert_eq!(
        decide("FIX", "후렴이 담긴 데모로 바꿔 주세요.").await.1["status"],
        "needs_fix"
    );
    let (_, mine) = call(
        &app,
        "POST",
        &format!("{base}/ar/submissions/{arid}/resubmit"),
        json!({"reply": "후렴까지 넣었어요."}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (mine["status"].as_str(), mine["fixReply"].as_str()),
        (Some("in_review"), Some("후렴까지 넣었어요."))
    );
    assert_eq!(decide("REVIEWED", "").await.1["status"], "reviewed");
    assert_eq!(decide("PROPOSING", "").await.1["status"], "proposing");
    let (_, closed) = decide("CLOSE", "").await;
    let steps: Vec<&str> = closed["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        steps,
        [
            "제출 완료",
            "심사 시작",
            "보완 요청",
            "보완 제출",
            "심사 완료",
            "제안 진행",
            "종료"
        ]
    );

    // Royalties: a statement with an unknown code is refused (preview says which line); the good one is stored.
    let rows = |work: &str| json!([{"work": work, "kind": "digital", "country": "KR", "collected": 100000}, {"work": code, "kind": "performance", "country": "JP", "collected": 20000}]);
    let (_, pv) = call(&app, "POST", "/api/staff/publishing/royalties", json!({"source": "KOMCA", "period": "2026-09", "status": "estimated", "rows": rows("AQW-999999"), "preview": true}), Some(&st)).await;
    assert_eq!(pv["errors"], 1, "{pv}");
    assert!(pv["rows"][0]["error"].is_string());
    let (s, _) = call(&app, "POST", "/api/staff/publishing/royalties", json!({"source": "KOMCA", "period": "2026-09", "status": "estimated", "rows": rows("AQW-999999")}), Some(&st)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, imp) = call(&app, "POST", "/api/staff/publishing/royalties", json!({"source": "KOMCA", "period": "2026-09", "status": "estimated", "fileName": "komca.csv", "rows": rows(&code)}), Some(&st)).await;
    assert_eq!(s, StatusCode::OK, "{imp}");
    assert_eq!(
        (
            imp["rows"][0]["fee"].as_i64(),
            imp["rows"][0]["net"].as_i64()
        ),
        (Some(10000), Some(90000)),
        "the contract's 10% commission"
    );
    let (_, mine) = call(
        &app,
        "GET",
        &format!("{base}/works/{id}/royalties"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(mine["items"].as_array().unwrap().len(), 2);
    assert_eq!(mine["items"][0]["status"], "estimated");
    let (_, sum) = call(
        &app,
        "GET",
        &format!("{base}/publishing/summary"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(sum["revenue"]["publishing"]["estimated"], 108000);
    let sid = imp["id"].as_str().unwrap();
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/royalties/{sid}/status"),
        json!({"status": "paid"}),
        Some(&st),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::UNPROCESSABLE_ENTITY,
        "estimated is confirmed before paid"
    );
    call(
        &app,
        "POST",
        &format!("/api/staff/publishing/royalties/{sid}/status"),
        json!({"status": "confirmed"}),
        Some(&st),
    )
    .await;
    // Paid means credited to the balance — only the staff console's posting does it (tests/staff_ops.rs).
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/staff/publishing/royalties/{sid}/status"),
        json!({"status": "paid"}),
        Some(&st),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, all) = call(
        &app,
        "GET",
        &format!("{base}/publishing/royalties"),
        json!({}),
        Some(&u),
    )
    .await;
    assert!(
        all["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "confirmed" && r["postedAt"].is_null())
    );
    let (_, sum) = call(
        &app,
        "GET",
        &format!("{base}/publishing/summary"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(sum["revenue"]["publishing"]["confirmed"], 108000);
    let (_, stl) = call(
        &app,
        "GET",
        "/api/staff/publishing/royalties",
        json!({}),
        Some(&st),
    )
    .await;
    assert_eq!(
        (
            stl["items"][0]["rowCount"].as_i64(),
            stl["items"][0]["totalCollected"].as_i64()
        ),
        (Some(2), Some(120000))
    );
    // Each answer reached the artist as a notice.
    let notices: Vec<String> =
        sqlx::query_scalar("SELECT title FROM portal.notifications WHERE org_id=$1")
            .bind(u.org)
            .fetch_all(&pool)
            .await
            .unwrap();
    for t in [
        "‘겨울 편지’ 지분 변경이 반려됐어요.",
        "‘겨울 편지’ 지분 변경이 반영됐어요.",
        "저작권 관리 계약이 연장됐어요.",
        "저작권 관리 계약 종료 요청이 반려됐어요.",
        "‘새벽 네 시’ A&R 보완 요청이 있어요.",
        "‘새벽 네 시’ A&R 심사가 끝났어요.",
        "새 저작권료 예상 내역이 들어왔어요.",
    ] {
        assert!(notices.iter().any(|n| n == t), "{t} in {notices:?}");
    }
}
