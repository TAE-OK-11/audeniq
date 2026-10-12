//! Electronic documents are sealed (Ed25519 + hash chain) when signed or
//! accepted; changing a document or a seal is detected; the monthly archive
//! stores the originals as sealed, under a signed, chained manifest, and the
//! chain head is anchored outside the database.
use audeniq_core::{archive, payout_keys::KeyRing, seal};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

mod support;
use support::*;

async fn keys() -> KeyRing {
    KeyRing::from_env(false).unwrap()
}

/// A signed agreement with an uploaded file, sealed like `sign_document` does.
async fn signed_agreement(pool: &PgPool, store: &MemStore, u: &User, release: Uuid) -> Uuid {
    let asset = Uuid::new_v4();
    let key = format!("registered/{}/{asset}/file", u.org);
    store.files.lock().await.insert(
        key.clone(),
        (
            b"%PDF-1.7 signed agreement".to_vec(),
            "application/pdf".into(),
        ),
    );
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(u.org)
        .bind(asset)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,state,sha256) VALUES($1,$2,'DOCUMENT',$3,25,'application/pdf','REGISTERED',$4)")
        .bind(asset).bind(u.org).bind(&key).bind(sha256_hex(b"%PDF-1.7 signed agreement"))
        .execute(pool).await.unwrap();
    let doc = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.documents(id,org_id,release_id,kind,title,body,status,signer_name,signature,signed_by,signed_at,asset_id,file_name)
                 VALUES($1,$2,$3,'AGREEMENT','배급 계약서','본문 제1조',
                        'SIGNED','홍길동','data:image/png;base64,AAAA',$4,now(),$5,'계약서.pdf')")
        .bind(doc).bind(u.org).bind(release).bind(u.user).bind(asset)
        .execute(pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    seal::seal(&mut tx, &keys().await, seal::Kind::Agreement, doc, "SIGNED")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    doc
}

#[sqlx::test]
async fn documents_are_sealed_verified_and_archived(pool: PgPool) {
    // Safety: this test binary only ever sets this value.
    unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32)) };
    let (app, store) = app(pool.clone()).await;
    let keys = keys().await;

    // Sign-up records and seals the terms version; without it, no account.
    let (s, v) = send(
        &app,
        "POST",
        "/api/auth/register",
        json!({"email": "noterms@example.test", "password": "Long-test-password-123!"}),
        None,
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "TERMS_ACCEPTANCE_REQUIRED");
    let (s, v) = send(&app, "POST", "/api/auth/register",
        json!({"email": "terms@example.test", "password": "Long-test-password-123!", "terms_version": "AUD-TERMS 2026.10.1", "privacy_notice_version": "AUD-PRIVACY 1.0-draft"}), None).await;
    assert_eq!(s, axum::http::StatusCode::OK, "{v}");
    let accepted: Vec<(String, String)> = sqlx::query_as(
        "SELECT doc, version FROM identity.terms_acceptances WHERE user_id=$1::uuid ORDER BY doc",
    )
    .bind(v["user_id"].as_str().unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        accepted,
        vec![
            ("PRIVACY_NOTICE".into(), "AUD-PRIVACY 1.0-draft".into()),
            ("TERMS".into(), "AUD-TERMS 2026.10.1".into())
        ]
    );
    assert!(
        sqlx::query("UPDATE identity.terms_acceptances SET version='x'")
            .execute(&pool)
            .await
            .is_err(),
        "acceptances are immutable"
    );

    let u = user(&app).await;
    let release = create_release(&app, &u).await;
    let doc = signed_agreement(&pool, &store, &u, release).await;

    // Every seal is signed with the published document key and chained.
    let mut c = pool.acquire().await.unwrap();
    let r = seal::verify(&mut c, &keys).await.unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.unsigned, 0);
    assert!(r.seals >= 4, "two sign-ups' consents and the agreement");
    let (_, pk) = send(&app, "GET", "/api/document-keys", Value::Null, None).await;
    assert_eq!(pk["keys"][0]["algorithm"], "Ed25519");
    let (version, sig, msg): (i16, Vec<u8>, String) = {
        let row: (i16, Vec<u8>, String, String, chrono::DateTime<chrono::Utc>, String) = sqlx::query_as(
            "SELECT key_version, signature, doc_kind, content_sha256, sealed_at, prev_hash FROM operations.document_seals WHERE doc_id=$1")
            .bind(doc).fetch_one(&pool).await.unwrap();
        let m = seal::message(
            &row.2,
            doc,
            "SIGNED",
            &row.3,
            &row.4.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            &row.5,
        );
        (row.0, row.1, m)
    };
    // Anyone can check a seal with the public key alone.
    let public = pk["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["version"] == version)
        .unwrap();
    let public = STANDARD
        .decode(public["public_key"].as_str().unwrap())
        .unwrap();
    assert!(
        aws_lc_rs::signature::UnparsedPublicKey::new(&aws_lc_rs::signature::ED25519, &public)
            .verify(msg.as_bytes(), &sig)
            .is_ok()
    );

    // The monthly archive: originals exactly as sealed, files byte for byte,
    // a signed manifest, and the chain head anchored outside the database.
    // An agreement signed before sealing existed: sealed (BACKFILL) and archived too.
    let legacy = Uuid::new_v4();
    let old_release = create_release(&app, &u).await;
    sqlx::query("INSERT INTO portal.documents(id,org_id,release_id,kind,title,body,status,signer_name,signature,signed_by,signed_at)
                 VALUES($1,$2,$3,'AGREEMENT','배급 계약서','예전 본문','SIGNED','홍길동','data:image/png;base64,AAAA',$4,now()-interval '1 year')")
        .bind(legacy).bind(u.org).bind(old_release).bind(u.user)
        .execute(&pool).await.unwrap();
    let archive_store = MemStore::default();
    let out = archive::run(
        &pool,
        store.as_ref(),
        &archive_store,
        &keys,
        "2026-09",
        "test-operator",
    )
    .await
    .unwrap();
    assert!(out["problems"].as_array().unwrap().is_empty(), "{out}");
    assert!(out["uploaded"].as_i64().unwrap() >= 4, "{out}");
    assert_eq!(out["backfilled_seals"], 1, "{out}");
    let event: String =
        sqlx::query_scalar("SELECT event FROM operations.document_seals WHERE doc_id=$1")
            .bind(legacy)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event, "BACKFILL");
    let files = archive_store.files.lock().await.clone();
    let manifest: Value = serde_json::from_slice(&files["manifests/2026-09.json"].0).unwrap();
    assert_eq!(manifest["format"], "audeniq-archive-manifest-v1");
    assert!(
        manifest["objects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["encrypted"] == true
                && o["object_key"].as_str().unwrap().ends_with(".sealed"))
    );
    let sig: Value = serde_json::from_slice(&files["manifests/2026-09.sig.json"].0).unwrap();
    let digest = seal::sha256_hex(&files["manifests/2026-09.json"].0);
    assert_eq!(sig["sha256"], digest);
    assert!(keys.verify_document(
        sig["key_version"].as_i64().unwrap() as i16,
        format!("audeniq-archive-manifest-v1|{digest}").as_bytes(),
        &STANDARD.decode(sig["signature"].as_str().unwrap()).unwrap()
    ));
    assert!(files.keys().any(|k| k.starts_with("anchors/")));
    // Objects are sealed: no names or signatures readable in the bucket.
    assert!(
        files
            .iter()
            .filter(|(k, _)| k.starts_with("documents/") || k.starts_with("files/"))
            .all(|(_, (b, _))| !String::from_utf8_lossy(b).contains("홍길동")
                && !b.windows(8).any(|w| w == b"%PDF-1.7"))
    );

    // 열람: the original comes back exactly as sealed.
    let entry = manifest["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["doc_id"] == doc.to_string() && o["object_kind"] == "DOCUMENT")
        .unwrap();
    let original = archive::open(
        &pool,
        &archive_store,
        &keys,
        "test-operator",
        entry["object_key"].as_str().unwrap(),
    )
    .await
    .unwrap();
    let (_, rendered) = seal::render(&mut c, seal::Kind::Agreement, doc)
        .await
        .unwrap();
    assert_eq!(original, rendered.as_bytes());
    let file = manifest["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["object_kind"] == "FILE")
        .unwrap();
    let pdf = archive::open(
        &pool,
        &archive_store,
        &keys,
        "test-operator",
        file["object_key"].as_str().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(pdf, b"%PDF-1.7 signed agreement");

    // The same month again changes nothing; the next month uploads only what changed
    // and chains its manifest to the previous one.
    let again = archive::run(
        &pool,
        store.as_ref(),
        &archive_store,
        &keys,
        "2026-09",
        "test-operator",
    )
    .await
    .unwrap();
    assert_eq!(again["already_completed"], true);
    let next = archive::run(
        &pool,
        store.as_ref(),
        &archive_store,
        &keys,
        "2026-10",
        "test-operator",
    )
    .await
    .unwrap();
    assert_eq!(next["uploaded"], 0, "{next}");
    let m2: Value =
        serde_json::from_slice(&archive_store.files.lock().await["manifests/2026-10.json"].0)
            .unwrap();
    assert_eq!(m2["previous_manifest_sha256"], digest);
    assert_eq!(
        m2["object_count"], manifest["object_count"],
        "each manifest is the full index"
    );

    let ok = archive::verify(&pool, &archive_store, &keys, true)
        .await
        .unwrap();
    assert_eq!(ok["ok"], true, "{ok}");

    // Tampering is detected: a changed document, an altered archive object,
    // and a rewritten seal (even with the immutability trigger removed).
    sqlx::query("UPDATE portal.documents SET body='본문 제1조 (몰래 고침)' WHERE id=$1")
        .bind(doc)
        .execute(&pool)
        .await
        .unwrap();
    let bad = archive::verify(&pool, &archive_store, &keys, true)
        .await
        .unwrap();
    assert!(
        bad["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["problem"] == "DOCUMENT_CHANGED"),
        "{bad}"
    );
    let changed = archive::run(
        &pool,
        store.as_ref(),
        &archive_store,
        &keys,
        "2026-11",
        "test-operator",
    )
    .await
    .unwrap();
    assert!(
        changed["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["problem"] == "DOCUMENT_CHANGED"),
        "a changed document is not archived as original"
    );
    sqlx::query("UPDATE portal.documents SET body='본문 제1조' WHERE id=$1")
        .bind(doc)
        .execute(&pool)
        .await
        .unwrap();

    archive_store
        .files
        .lock()
        .await
        .get_mut(entry["object_key"].as_str().unwrap())
        .unwrap()
        .0
        .push(0);
    let bad = archive::verify(&pool, &archive_store, &keys, true)
        .await
        .unwrap();
    assert!(
        bad["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["problem"] == "ARCHIVE_OBJECT_CHANGED"),
        "{bad}"
    );
    assert!(
        archive::open(
            &pool,
            &archive_store,
            &keys,
            "test-operator",
            entry["object_key"].as_str().unwrap()
        )
        .await
        .is_err()
    );

    sqlx::query("ALTER TABLE operations.document_seals DISABLE TRIGGER immutable")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE operations.document_seals SET content_sha256=repeat('0',64) WHERE doc_id=$1",
    )
    .bind(doc)
    .execute(&pool)
    .await
    .unwrap();
    let r = seal::verify(&mut c, &keys).await.unwrap();
    assert!(
        r.problems.iter().any(|p| p["problem"] == "SEAL_ALTERED"),
        "{:?}",
        r.problems
    );
    // Rebuilding the whole chain consistently still differs from the anchor in WORM.
    sqlx::query("UPDATE operations.document_seals SET hash=md5(hash)||md5(hash) WHERE seq=(SELECT max(seq) FROM operations.document_seals)").execute(&pool).await.unwrap();
    let bad = archive::verify(&pool, &archive_store, &keys, false)
        .await
        .unwrap();
    assert!(
        bad["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["problem"] == "CHAIN_DIFFERS_FROM_ANCHOR"),
        "{bad}"
    );
    sqlx::query("ALTER TABLE operations.document_seals ENABLE TRIGGER immutable")
        .execute(&pool)
        .await
        .unwrap();

    // Retention: only expired objects are purged, and records only gain a purge mark.
    assert!(
        sqlx::query("DELETE FROM operations.archive_objects")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE operations.archive_objects SET object_key='x'")
            .execute(&pool)
            .await
            .is_err()
    );
    let none = archive::purge_expired(&pool, &archive_store, "test-operator")
        .await
        .unwrap();
    assert_eq!(none["purged"], 0);
    sqlx::query("ALTER TABLE operations.archive_objects DISABLE TRIGGER guard")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE operations.archive_objects SET retain_until=current_date-1 WHERE object_kind='FILE'").execute(&pool).await.unwrap();
    sqlx::query("ALTER TABLE operations.archive_objects ENABLE TRIGGER guard")
        .execute(&pool)
        .await
        .unwrap();
    let purged = archive::purge_expired(&pool, &archive_store, "test-operator")
        .await
        .unwrap();
    assert_eq!(purged["purged"], 1);
    assert!(
        !archive_store
            .files
            .lock()
            .await
            .contains_key(file["object_key"].as_str().unwrap())
    );
}

fn sha256_hex(b: &[u8]) -> String {
    seal::sha256_hex(b)
}
