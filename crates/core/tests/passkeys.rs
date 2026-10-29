//! Passkey sign-in with a software authenticator: register (with the current
//! password), sign in with the email + passkey instead of the password, and
//! the checks that refuse replays, wrong accounts, origins and clones.
use audeniq_core::passkey;
use aws_lc_rs::{
    encoding::{AsDer, PublicKeyX509Der},
    rand::SystemRandom,
    signature::{self, EcdsaKeyPair, Ed25519KeyPair, KeyPair},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

mod support;
use support::*;

const PASSWORD: &str = "Long-test-password-123!";
const RP: &str = "localhost";
const ADMIN: &str = "http://admin.localhost:5174";

enum Key {
    Ec(EcdsaKeyPair),
    Ed(Ed25519KeyPair),
}

/// A software authenticator holding one credential.
struct Authenticator {
    key: Key,
    id: Vec<u8>,
    counter: u32,
}

impl Authenticator {
    fn ec() -> Self {
        Self {
            key: Key::Ec(
                EcdsaKeyPair::generate(&signature::ECDSA_P256_SHA256_ASN1_SIGNING).unwrap(),
            ),
            id: Uuid::new_v4().as_bytes().to_vec(),
            counter: 0,
        }
    }
    /// Synced passkeys report a counter of 0 throughout.
    fn ed_synced() -> Self {
        Self {
            key: Key::Ed(Ed25519KeyPair::generate().unwrap()),
            id: Uuid::new_v4().as_bytes().to_vec(),
            counter: 0,
        }
    }
    fn alg(&self) -> i32 {
        match self.key {
            Key::Ec(_) => -7,
            Key::Ed(_) => -8,
        }
    }
    fn spki(&self) -> Vec<u8> {
        match &self.key {
            Key::Ec(k) => AsDer::<PublicKeyX509Der>::as_der(k.public_key())
                .unwrap()
                .as_ref()
                .to_vec(),
            Key::Ed(k) => AsDer::<PublicKeyX509Der>::as_der(k.public_key())
                .unwrap()
                .as_ref()
                .to_vec(),
        }
    }
    fn auth_data(&self, flags: u8, attested: bool) -> Vec<u8> {
        let mut d = Sha256::digest(RP.as_bytes()).to_vec();
        d.push(flags);
        d.extend_from_slice(&self.counter.to_be_bytes());
        if attested {
            d.extend_from_slice(&[0u8; 16]);
            d.extend_from_slice(&(self.id.len() as u16).to_be_bytes());
            d.extend_from_slice(&self.id);
            d.extend_from_slice(b"\xa5cose-key-not-parsed");
        }
        d
    }
    fn sign(&self, msg: &[u8]) -> Vec<u8> {
        match &self.key {
            Key::Ec(k) => k.sign(&SystemRandom::new(), msg).unwrap().as_ref().to_vec(),
            Key::Ed(k) => k.sign(msg).as_ref().to_vec(),
        }
    }
}

fn client_data(kind: &str, challenge: &str, origin: &str) -> Vec<u8> {
    json!({"type": kind, "challenge": challenge, "origin": origin, "crossOrigin": false})
        .to_string()
        .into_bytes()
}

async fn register(
    app: &Router,
    u: &User,
    a: &Authenticator,
    password: &str,
) -> (StatusCode, Value) {
    enroll(app, u, a, password, None).await
}

async fn register_with_proof(
    app: &Router,
    u: &User,
    a: &Authenticator,
    proof: &str,
) -> (StatusCode, Value) {
    enroll(app, u, a, "", Some(proof)).await
}

async fn enroll(
    app: &Router,
    u: &User,
    a: &Authenticator,
    password: &str,
    proof: Option<&str>,
) -> (StatusCode, Value) {
    let (s, o) = send(
        app,
        "POST",
        "/api/auth/passkeys/options",
        json!({}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{o}");
    let pk = &o["public_key"];
    assert_eq!(pk["rp"]["id"], RP);
    assert_eq!(pk["authenticatorSelection"]["userVerification"], "required");
    assert_eq!(pk["user"]["id"], B64.encode(u.user.as_bytes()));
    let cdj = client_data("webauthn.create", pk["challenge"].as_str().unwrap(), ORIGIN);
    send(
        app,
        "POST",
        "/api/auth/passkeys/register",
        json!({
            "challenge_id": o["challenge_id"],
            "current_password": password,
            "reauth_token": proof,
            "name": "테스트 기기",
            "credential_id": B64.encode(&a.id),
            "client_data_json": B64.encode(&cdj),
            "authenticator_data": B64.encode(a.auth_data(0x45, true)),
            "public_key": B64.encode(a.spki()),
            "alg": a.alg(),
        }),
        Some(u),
    )
    .await
}

struct Assertion {
    flags: u8,
    origin: &'static str,
    admin: bool,
    email: Option<String>,
    user_handle: Option<String>,
}

impl Default for Assertion {
    fn default() -> Self {
        Self {
            flags: 0x05,
            origin: ORIGIN,
            admin: false,
            email: None,
            user_handle: None,
        }
    }
}

/// Signs in; returns the status, body and session cookie.
async fn login(
    app: &Router,
    a: &Authenticator,
    x: Assertion,
) -> (StatusCode, Value, Option<String>) {
    let post = |path: &'static str, body: Value| {
        let mut b = Request::builder()
            .method("POST")
            .uri(path)
            .header("x-audeniq-service", SECRET)
            .header("content-type", "application/json");
        b = if x.admin {
            b.header("origin", ADMIN).header("x-audeniq-site", "admin")
        } else {
            b.header("origin", ORIGIN)
        };
        app.clone()
            .oneshot(b.body(Body::from(body.to_string())).unwrap())
    };
    let resp = post("/api/auth/passkey/options", json!({})).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let o: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        o["public_key"]["allowCredentials"],
        json!([]),
        "no account hints"
    );
    assert_eq!(o["public_key"]["rpId"], RP);
    let cdj = client_data(
        "webauthn.get",
        o["public_key"]["challenge"].as_str().unwrap(),
        x.origin,
    );
    let ad = a.auth_data(x.flags, false);
    let mut msg = ad.clone();
    msg.extend_from_slice(&Sha256::digest(&cdj));
    let body = json!({
        "challenge_id": o["challenge_id"],
        "email": x.email,
        "credential_id": B64.encode(&a.id),
        "client_data_json": B64.encode(&cdj),
        "authenticator_data": B64.encode(&ad),
        "signature": B64.encode(a.sign(&msg)),
        "user_handle": x.user_handle,
    });
    let resp = post("/api/auth/passkey/login", body).await.unwrap();
    let status = resp.status();
    let cookie = resp
        .headers()
        .get("set-cookie")
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

async fn step_up(app: &Router, u: &User, a: &Authenticator) -> Value {
    let (status, o) = send(
        app,
        "POST",
        "/api/auth/passkey/reauth/options",
        json!({}),
        Some(u),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{o}");
    let cdj = client_data(
        "webauthn.get",
        o["public_key"]["challenge"].as_str().unwrap(),
        ORIGIN,
    );
    let ad = a.auth_data(0x05, false);
    let mut msg = ad.clone();
    msg.extend_from_slice(&Sha256::digest(&cdj));
    json!({"challenge_id":o["challenge_id"],"credential_id":B64.encode(&a.id),
        "client_data_json":B64.encode(cdj),"authenticator_data":B64.encode(ad),"signature":B64.encode(a.sign(&msg))})
}

#[sqlx::test]
async fn sign_in_with_email_and_passkey_instead_of_password(pool: PgPool) {
    // Safety: this test binary only ever sets these values.
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("STAFF_MFA_REQUIRED", "true");
        // One passkey for both sites: the RP ID is their common parent.
        std::env::set_var("ADMIN_ORIGIN", ADMIN);
        std::env::set_var("PASSKEY_RP_ID", RP);
    }
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let email = audeniq_core::pii::user_email(&keys, &pool, u.user)
        .await
        .unwrap();
    let mut a = Authenticator::ec();

    // Adding a passkey needs the current password.
    let (s, v) = register(&app, &u, &a, "Wrong-password-123456").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "CURRENT_PASSWORD_INVALID");
    let (s, v) = register(&app, &u, &a, PASSWORD).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["items"][0]["name"], "테스트 기기");
    // The same credential cannot be registered twice.
    let (_, v) = register(&app, &u, &a, PASSWORD).await;
    assert_eq!(v["error"]["code"], "PASSKEY_ALREADY_REGISTERED");

    // Email first, then the passkey: signed in, no password.
    a.counter = 1;
    let (s, v, cookie) = login(
        &app,
        &a,
        Assertion {
            email: Some(email.to_uppercase()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let session = User {
        cookie: cookie.unwrap(),
        csrf: v["csrf_token"].as_str().unwrap().into(),
        ..u.clone()
    };
    let (s, me) = send(&app, "GET", "/api/me", Value::Null, Some(&session)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["user_id"], u.user.to_string());
    // A passkey with user verification is multi-factor.
    let mfa: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.sessions WHERE user_id=$1 AND mfa_at IS NOT NULL",
    )
    .bind(u.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(mfa, 1);

    // Refused: another account's email, a non-advancing counter (clone),
    // no user verification, another origin, a wrong user handle.
    a.counter = 2;
    let other = user(&app).await;
    let other_email = audeniq_core::pii::user_email(&keys, &pool, other.user)
        .await
        .unwrap();
    let (s, _, c) = login(
        &app,
        &a,
        Assertion {
            email: Some(other_email),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(c.is_none());
    a.counter = 1;
    let (s, _, _) = login(&app, &a, Assertion::default()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "counter must advance");
    a.counter = 3;
    let (s, _, _) = login(
        &app,
        &a,
        Assertion {
            flags: 0x01,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "user verification required");
    let (s, _, _) = login(
        &app,
        &a,
        Assertion {
            origin: "https://evil.example",
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(
        &app,
        &a,
        Assertion {
            user_handle: Some(B64.encode(other.user.as_bytes())),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A key that is not registered.
    let stranger = Authenticator::ec();
    let (s, _, _) = login(&app, &stranger, Assertion::default()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // Without an email (account picked from the passkey) also works.
    let (s, v, _) = login(
        &app,
        &a,
        Assertion {
            user_handle: Some(B64.encode(u.user.as_bytes())),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // A challenge is single use.
    let (_, o) = send(&app, "POST", "/api/auth/passkey/options", json!({}), None).await;
    let cdj = client_data(
        "webauthn.get",
        o["public_key"]["challenge"].as_str().unwrap(),
        ORIGIN,
    );
    a.counter = 10;
    let ad = a.auth_data(0x05, false);
    let mut msg = ad.clone();
    msg.extend_from_slice(&Sha256::digest(&cdj));
    let body = json!({"challenge_id": o["challenge_id"], "credential_id": B64.encode(&a.id),
        "client_data_json": B64.encode(&cdj), "authenticator_data": B64.encode(&ad), "signature": B64.encode(a.sign(&msg))});
    let (s, _) = send(&app, "POST", "/api/auth/passkey/login", body.clone(), None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = send(&app, "POST", "/api/auth/passkey/login", body, None).await;
    assert_eq!(v["error"]["code"], "PASSKEY_CHALLENGE_INVALID");

    // Staff signing in with a passkey skip the TOTP step; a password session does not.
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')")
        .bind(u.user).execute(&pool).await.unwrap();
    let (s, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&session)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // The admin console (its own origin) signs in with the same passkey.
    a.counter = 20;
    let (s, v, _) = login(
        &app,
        &a,
        Assertion {
            admin: true,
            origin: ADMIN,
            email: Some(email.clone()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&u)).await;
    assert_eq!(v["error"]["code"], "STAFF_MFA_ENROLL_REQUIRED");

    // A synced Ed25519 passkey (counter stays 0); then removal ends it.
    let synced = Authenticator::ed_synced();
    let (s, v) = register(&app, &u, &synced, PASSWORD).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v, _) = login(
        &app,
        &synced,
        Assertion {
            email: Some(email.clone()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v, _) = login(
        &app,
        &synced,
        Assertion {
            email: Some(email.clone()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = send(&app, "GET", "/api/auth/passkeys", Value::Null, Some(&u)).await;
    let id = list["items"][1]["id"].as_str().unwrap().to_string();
    let (_, v) = send(
        &app,
        "DELETE",
        &format!("/api/auth/passkeys/{id}"),
        Value::Null,
        Some(&other),
    )
    .await;
    assert_eq!(v["error"]["code"], "NOT_FOUND", "only the owner removes");
    let (s, v) = send(
        &app,
        "DELETE",
        &format!("/api/auth/passkeys/{id}"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    let (s, _, _) = login(&app, &synced, Assertion::default()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action||':'||reason_code FROM operations.audit_events WHERE action LIKE 'auth.%' AND (resource_id=$1 OR reason_code='PASSKEY')",
    )
    .bind(u.user).fetch_all(&pool).await.unwrap();
    for a in [
        "auth.passkey.added:WEBAUTHN",
        "auth.login:PASSKEY",
        "auth.login_failed:PASSKEY",
        "auth.passkey.removed:WEBAUTHN",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a}: {actions:?}");
    }
    // The last passkey may be removed when a real emergency password remains.
    let (_, list) = send(&app, "GET", "/api/auth/passkeys", Value::Null, Some(&u)).await;
    let last = list["items"][0]["id"].as_str().unwrap();
    let (status, v) = send(
        &app,
        "DELETE",
        &format!("/api/auth/passkeys/{last}"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["items"].as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn signup_profiles_optional_passkeys_and_fresh_reauthentication(pool: PgPool) {
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("PASSKEY_RP_ID", RP);
        std::env::set_var("ADMIN_ORIGIN", ADMIN);
    }
    let (app, _) = app(pool.clone()).await;
    for (account_type, org_kind, party_kind) in [
        ("personal", "PERSONAL", "PERSON"),
        ("label", "LABEL", "LEGAL_ENTITY"),
    ] {
        let email = format!("{}@example.test", Uuid::new_v4());
        let (status, v) = send(
            &app,
            "POST",
            "/api/auth/register",
            json!({
                "email":email,"password":PASSWORD,"terms_version":"AUD-TERMS 2026.10.1",
                "name":"Sample Artist","phone":"010-1234-5678","account_type":account_type
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let user: Uuid = v["user_id"].as_str().unwrap().parse().unwrap();
        let stored: (String, String, String, String, String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
            "SELECT o.kind,p.kind,p.display_name,o.name,ph.phone_last4,ph.verified_at FROM identity.users u JOIN identity.parties p ON p.id=u.party_id JOIN identity.orgs o ON o.id=p.org_id JOIN portal.account_phones ph ON ph.user_id=u.id WHERE u.id=$1")
            .bind(user).fetch_one(&pool).await.unwrap();
        assert_eq!(
            stored,
            (
                org_kind.into(),
                party_kind.into(),
                "Sample Artist".into(),
                "Sample Artist".into(),
                "5678".into(),
                None
            )
        );
        let phone_hash: String =
            sqlx::query_scalar("SELECT phone_hash FROM portal.account_phones WHERE user_id=$1")
                .bind(user)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            phone_hash,
            audeniq_core::sms::phone_hash(SECRET, "01012345678")
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM identity.passkeys WHERE user_id=$1")
                .bind(user)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "skipping enrollment completes signup");
        let (status, v) = send(
            &app,
            "POST",
            "/api/auth/login",
            json!({"email":email,"password":PASSWORD}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
    }
    for profile in [
        json!({"name":" "}),
        json!({"name":"x".repeat(201)}),
        json!({"phone":"invalid"}),
        json!({"account_type":"company"}),
    ] {
        let mut body = json!({"email":format!("{}@example.test", Uuid::new_v4()),"password":PASSWORD,"terms_version":"AUD-TERMS 2026.10.1"});
        body.as_object_mut()
            .unwrap()
            .extend(profile.as_object().unwrap().clone());
        let (status, _) = send(&app, "POST", "/api/auth/register", body, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    // The helper signs up without profile fields, then uses the emergency password.
    let u = user(&app).await;
    let first = Authenticator::ed_synced();
    assert_eq!(register(&app, &u, &first, PASSWORD).await.0, StatusCode::OK);
    let (status, v, cookie) = login(&app, &first, Assertion::default()).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let session = User {
        cookie: cookie.unwrap(),
        csrf: v["csrf_token"].as_str().unwrap().into(),
        ..u.clone()
    };
    let assertion = step_up(&app, &session, &first).await;
    let (status, proof) = send(
        &app,
        "POST",
        "/api/auth/passkey/reauth",
        assertion.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proof}");
    let (_, replay) = send(
        &app,
        "POST",
        "/api/auth/passkey/reauth",
        assertion,
        Some(&session),
    )
    .await;
    assert_eq!(replay["error"]["code"], "PASSKEY_CHALLENGE_INVALID");
    // A challenge issued to one session cannot be completed in another.
    let bound = step_up(&app, &session, &first).await;
    let (_, denied) = send(&app, "POST", "/api/auth/passkey/reauth", bound, Some(&u)).await;
    assert_eq!(denied["error"]["code"], "PASSKEY_CHALLENGE_INVALID");
    let expired = step_up(&app, &session, &first).await;
    sqlx::query(
        "UPDATE identity.webauthn_challenges SET expires_at=now()-interval '1 second' WHERE id=$1",
    )
    .bind(
        expired["challenge_id"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap(),
    )
    .execute(&pool)
    .await
    .unwrap();
    let (_, denied) = send(
        &app,
        "POST",
        "/api/auth/passkey/reauth",
        expired,
        Some(&session),
    )
    .await;
    assert_eq!(denied["error"]["code"], "PASSKEY_CHALLENGE_INVALID");
    let token = proof["reauth_token"].as_str().unwrap();
    let second = Authenticator::ed_synced();
    let (_, wrong_session) = register_with_proof(&app, &u, &second, token).await;
    assert_eq!(wrong_session["error"]["code"], "PASSKEY_STEP_UP_REQUIRED");
    let (status, v) = register_with_proof(&app, &session, &second, token).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (_, replay) = register_with_proof(&app, &session, &Authenticator::ec(), token).await;
    assert_eq!(replay["error"]["code"], "PASSKEY_STEP_UP_REQUIRED");
    let assertion = step_up(&app, &session, &first).await;
    let (_, proof) = send(
        &app,
        "POST",
        "/api/auth/passkey/reauth",
        assertion,
        Some(&session),
    )
    .await;
    let expired_token = proof["reauth_token"].as_str().unwrap();
    sqlx::query("UPDATE identity.passkey_proofs SET expires_at=now()-interval '1 second' WHERE token_hash=$1")
        .bind(audeniq_core::auth::hash_token(expired_token)).execute(&pool).await.unwrap();
    let (_, denied) =
        register_with_proof(&app, &session, &Authenticator::ec(), expired_token).await;
    assert_eq!(denied["error"]["code"], "PASSKEY_STEP_UP_REQUIRED");
    // Passkey sessions can access settings, revoke sessions and logout without a password.
    assert_eq!(
        send(&app, "GET", "/api/me/account", Value::Null, Some(&session))
            .await
            .0,
        StatusCode::OK
    );
    let (status, listed) = send(
        &app,
        "GET",
        "/api/auth/sessions",
        Value::Null,
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let other = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["current"] == false)
        .unwrap();
    let path = format!(
        "/api/auth/sessions/{}/revoke",
        other["id"].as_str().unwrap()
    );
    assert_eq!(
        send(&app, "POST", &path, json!({}), Some(&session)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "GET", "/api/me", Value::Null, Some(&u)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, list) = send(
        &app,
        "GET",
        "/api/auth/passkeys",
        Value::Null,
        Some(&session),
    )
    .await;
    let path = format!(
        "/api/auth/passkeys/{}",
        list["items"][0]["id"].as_str().unwrap()
    );
    let (status, renamed) = send(
        &app,
        "PATCH",
        &path,
        json!({"name":"Travel key"}),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["items"][0]["name"], "Travel key");
    sqlx::query("UPDATE identity.users SET password_hash='!oauth' WHERE id=$1")
        .bind(u.user)
        .execute(&pool)
        .await
        .unwrap();
    let (_, list) = send(
        &app,
        "GET",
        "/api/auth/passkeys",
        Value::Null,
        Some(&session),
    )
    .await;
    let ids: Vec<String> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap().into())
        .collect();
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/auth/passkeys/{}", ids[0]),
            Value::Null,
            Some(&session)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, blocked) = send(
        &app,
        "DELETE",
        &format!("/api/auth/passkeys/{}", ids[1]),
        Value::Null,
        Some(&session),
    )
    .await;
    assert_eq!(blocked["error"]["code"], "PASSKEY_LAST_SIGN_IN_METHOD");
    let oauth_id = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.oauth_identities(id,provider,subject,user_id) VALUES($1,'google',$2,$3)")
        .bind(oauth_id).bind(Uuid::new_v4().to_string()).bind(u.user).execute(&pool).await.unwrap();
    let passkey_path = format!("/api/auth/passkeys/{}", ids[1]);
    let oauth_path = format!("/api/auth/oauth/identities/{oauth_id}");
    let (passkey_result, oauth_result) = tokio::join!(
        send(&app, "DELETE", &passkey_path, Value::Null, Some(&session)),
        send(&app, "DELETE", &oauth_path, Value::Null, Some(&session))
    );
    assert_eq!(
        usize::from(passkey_result.0 == StatusCode::OK)
            + usize::from(oauth_result.0 == StatusCode::OK),
        1,
        "concurrent removals must preserve one method: {passkey_result:?}, {oauth_result:?}"
    );
    let remaining: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM identity.passkeys WHERE user_id=$1)+(SELECT count(*) FROM identity.oauth_identities WHERE user_id=$1)")
        .bind(u.user).fetch_one(&pool).await.unwrap();
    assert_eq!(remaining, 1);
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/auth/logout-all",
            json!({}),
            Some(&session)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/auth/sessions",
            Value::Null,
            Some(&session)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let emergency: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE resource_id=$1 AND action='auth.emergency_password_login'").bind(u.user).fetch_one(&pool).await.unwrap();
    assert!(emergency > 0);
}

#[test]
fn authenticator_data_checks_rp_and_reads_the_credential_id() {
    let a = Authenticator::ec();
    let d = a.auth_data(0x45, true);
    let parsed = passkey::auth_data(&d, RP).unwrap();
    assert_eq!(parsed.credential_id, Some(a.id.as_slice()));
    assert!(passkey::auth_data(&d, "evil.example").is_none());
    assert!(passkey::auth_data(&d[..36], RP).is_none());
    // A declared credential id longer than the data.
    let mut short = d[..60].to_vec();
    short[53] = 0xff;
    assert!(passkey::auth_data(&short, RP).is_none());
}
