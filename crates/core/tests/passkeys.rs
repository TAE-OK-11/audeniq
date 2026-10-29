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
    aaguid: Uuid,
}

impl Authenticator {
    fn ec() -> Self {
        Self {
            key: Key::Ec(
                EcdsaKeyPair::generate(&signature::ECDSA_P256_SHA256_ASN1_SIGNING).unwrap(),
            ),
            id: Uuid::new_v4().as_bytes().to_vec(),
            counter: 0,
            aaguid: Uuid::nil(),
        }
    }
    /// Synced passkeys report a counter of 0 throughout.
    fn ed_synced() -> Self {
        Self {
            key: Key::Ed(Ed25519KeyPair::generate().unwrap()),
            id: Uuid::new_v4().as_bytes().to_vec(),
            counter: 0,
            aaguid: Uuid::nil(),
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
            d.extend_from_slice(self.aaguid.as_bytes());
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
            flags: 0x04,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "user presence required");
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

#[sqlx::test]
async fn provider_names_are_stored_and_listed_without_exposing_credentials(pool: PgPool) {
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("PASSKEY_RP_ID", RP);
        std::env::set_var("ADMIN_ORIGIN", ADMIN);
    }
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    for (aaguid, expected) in [
        ("bada5566-a7aa-401f-bd96-45619a55120d", Some("1Password")),
        ("d548826e-79b4-db40-a3d8-11116f7e8349", Some("Bitwarden")),
        ("50726f74-6f6e-5061-7373-50726f746f6e", Some("Proton Pass")),
        ("00000000-0000-0000-0000-000000000001", None),
        ("00000000-0000-0000-0000-000000000000", None),
    ] {
        let mut a = Authenticator::ed_synced();
        a.aaguid = aaguid.parse().unwrap();
        let (status, v) = register(&app, &u, &a, PASSWORD).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let stored: (Uuid, String, Option<Uuid>, Option<String>) = sqlx::query_as(
            "SELECT id,name,aaguid,authenticator_name FROM identity.passkeys WHERE credential_id=$1")
            .bind(&a.id).fetch_one(&pool).await.unwrap();
        assert_eq!(stored.1, expected.unwrap_or("테스트 기기"));
        assert_eq!(stored.2, (!a.aaguid.is_nil()).then_some(a.aaguid));
        assert_eq!(stored.3.as_deref(), expected);
        let (status, list) = send(&app, "GET", "/api/auth/passkeys", Value::Null, Some(&u)).await;
        assert_eq!(status, StatusCode::OK);
        let item = list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == stored.0.to_string())
            .unwrap();
        assert_eq!(item["authenticator_name"], json!(expected));
        assert_eq!(item["name"], stored.1);
        for field in ["aaguid", "credential_id", "public_key"] {
            assert!(item.get(field).is_none());
        }
        // A custom rename remains compatible while the independent provider hint survives.
        let (status, renamed) = send(
            &app,
            "PATCH",
            &format!("/api/auth/passkeys/{}", stored.0),
            json!({"name":"Custom nickname"}),
            Some(&u),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{renamed}");
        let item = renamed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == stored.0.to_string())
            .unwrap();
        assert_eq!(item["name"], "Custom nickname");
        assert_eq!(item["authenticator_name"], json!(expected));
    }
    // Historic rows remain readable, without inferring a provider from their name.
    sqlx::query(
        "UPDATE identity.passkeys SET aaguid=NULL,authenticator_name=NULL WHERE user_id=$1",
    )
    .bind(u.user)
    .execute(&pool)
    .await
    .unwrap();
    let (_, list) = send(&app, "GET", "/api/auth/passkeys", Value::Null, Some(&u)).await;
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["authenticator_name"].is_null())
    );
}

#[sqlx::test]
async fn challenge_cleanup_is_bounded_skips_locks_and_preserves_live_hashes(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    sqlx::query("INSERT INTO identity.webauthn_challenges(id,challenge_hash,purpose,expires_at) SELECT gen_random_uuid(),decode(repeat('ab',32),'hex'),'LOGIN',now()-interval '2 hours' FROM generate_series(1,600)")
        .execute(&pool).await.unwrap();
    let mut holder = pool.begin().await.unwrap();
    let held: Uuid =
        sqlx::query_scalar("SELECT id FROM identity.webauthn_challenges LIMIT 1 FOR UPDATE")
            .fetch_one(&mut *holder)
            .await
            .unwrap();
    let (status, o) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        send(&app, "POST", "/api/auth/passkey/options", json!({}), None),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{o}");
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.webauthn_challenges WHERE expires_at<now()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 600 - 256);
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity.webauthn_challenges WHERE id=$1)")
            .bind(held)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(exists);
    let (hash, seconds): (Vec<u8>, f64) = sqlx::query_as("SELECT challenge_hash,extract(epoch FROM expires_at-clock_timestamp())::float8 FROM identity.webauthn_challenges WHERE id=$1")
        .bind(o["challenge_id"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(
        hash,
        Sha256::digest(o["public_key"]["challenge"].as_str().unwrap().as_bytes()).to_vec()
    );
    assert!(seconds > 295.0 && seconds <= 300.0);
    holder.rollback().await.unwrap();
}

#[sqlx::test]
async fn registration_rejects_a_challenge_expiring_while_waiting_for_account_lock(pool: PgPool) {
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("PASSKEY_RP_ID", RP);
        std::env::set_var("ADMIN_ORIGIN", ADMIN);
    }
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let first = Authenticator::ed_synced();
    assert_eq!(register(&app, &u, &first, PASSWORD).await.0, StatusCode::OK);
    let assertion = step_up(&app, &u, &first).await;
    let (status, proof) = send(
        &app,
        "POST",
        "/api/auth/passkey/reauth",
        assertion,
        Some(&u),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{proof}");
    let (status, o) = send(
        &app,
        "POST",
        "/api/auth/passkeys/options",
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{o}");
    let next = Authenticator::ed_synced();
    let cdj = client_data(
        "webauthn.create",
        o["public_key"]["challenge"].as_str().unwrap(),
        ORIGIN,
    );
    let body = json!({"challenge_id":o["challenge_id"],"reauth_token":proof["reauth_token"],
        "name":"Test device","credential_id":B64.encode(&next.id),"client_data_json":B64.encode(cdj),
        "authenticator_data":B64.encode(next.auth_data(0x45,true)),"public_key":B64.encode(next.spki()),"alg":next.alg()});
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
        .bind(u.user)
        .execute(&mut *holder)
        .await
        .unwrap();
    let pending_app = app.clone();
    let pending_user = u.clone();
    let pending = tokio::spawn(async move {
        send(
            &pending_app,
            "POST",
            "/api/auth/passkeys/register",
            body,
            Some(&pending_user),
        )
        .await
    });
    // Observe the actual database lock wait, rather than guessing when the request began.
    let transaction_start: chrono::DateTime<chrono::Utc> = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let started: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
                "SELECT xact_start FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query='SELECT id FROM identity.users WHERE id=$1 FOR UPDATE' LIMIT 1")
                .fetch_optional(&pool).await.unwrap();
            if let Some(started) = started { break started; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let expired_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "UPDATE identity.webauthn_challenges SET expires_at=clock_timestamp()-interval '1 millisecond' WHERE id=$1 RETURNING expires_at")
        .bind(o["challenge_id"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(&pool).await.unwrap();
    assert!(
        expired_at > transaction_start,
        "the original transaction clock would accept it"
    );
    holder.commit().await.unwrap();
    let (status, denied) = pending.await.unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{denied}");
    assert_eq!(denied["error"]["code"], "PASSKEY_CHALLENGE_INVALID");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.passkeys WHERE user_id=$1")
        .bind(u.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
