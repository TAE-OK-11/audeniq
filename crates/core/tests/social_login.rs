//! Google / Apple sign-in against a local stand-in for both providers
//! (authorization code + PKCE, signed ID tokens, Apple's form POST return).
use aws_lc_rs::{
    rand::SystemRandom,
    rsa::{KeySize, PublicKeyComponents},
    signature::{self, EcdsaKeyPair, KeyPair, RsaKeyPair},
};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use uuid::Uuid;

mod support;
use support::*;

const GOOGLE_ID: &str = "google-client.apps.example";
const GOOGLE_SECRET: &str = "google-test-secret";
const APPLE_ID: &str = "com.audeniq.studio.signin";

struct Grant {
    challenge: String,
    nonce: String,
    sub: String,
    email: Option<String>,
    verified: Value,
}

#[derive(Clone)]
struct Fixture {
    pool: PgPool,
    revoke_link: Arc<Mutex<bool>>,
    base: String,
    grants: Arc<Mutex<HashMap<String, Grant>>>,
    rsa: Arc<RsaKeyPair>,
    apple_public: Vec<u8>,
    /// Overrides the nonce the token carries (to test the nonce check).
    forged_nonce: Arc<Mutex<Option<String>>>,
}

fn form(body: &[u8]) -> HashMap<String, String> {
    url::form_urlencoded::parse(body).into_owned().collect()
}

impl Fixture {
    fn id_token(&self, provider: &str, g: &Grant, aud: &str) -> String {
        let header = B64.encode(json!({"alg": "RS256", "kid": "k1"}).to_string());
        let nonce = self
            .forged_nonce
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(g.nonce.clone());
        let now = chrono::Utc::now().timestamp();
        let claims = B64.encode(
            json!({"iss": format!("{}/{provider}", self.base), "aud": aud, "sub": g.sub, "iat": now,
                   "exp": now + 600, "nonce": nonce, "email": g.email, "email_verified": g.verified})
            .to_string(),
        );
        let input = format!("{header}.{claims}");
        let mut sig = vec![0; self.rsa.public_modulus_len()];
        self.rsa
            .sign(
                &signature::RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                input.as_bytes(),
                &mut sig,
            )
            .unwrap();
        format!("{input}.{}", B64.encode(sig))
    }

    /// The token endpoint: checks the client, redirect URI and PKCE verifier.
    fn token(&self, provider: &str, f: &HashMap<String, String>) -> Result<String, StatusCode> {
        let (id, ok_secret) = match provider {
            "google" => (GOOGLE_ID, f["client_secret"] == GOOGLE_SECRET),
            _ => (APPLE_ID, self.apple_secret_ok(&f["client_secret"])),
        };
        let g = self
            .grants
            .lock()
            .unwrap()
            .remove(&f["code"])
            .ok_or(StatusCode::BAD_REQUEST)?;
        let pkce = B64.encode(Sha256::digest(f["code_verifier"].as_bytes())) == g.challenge;
        let redirect = f["redirect_uri"] == format!("{ORIGIN}/api/auth/oauth/{provider}/callback");
        if f["client_id"] != id
            || !ok_secret
            || !pkce
            || !redirect
            || f["grant_type"] != "authorization_code"
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(
            json!({"id_token": self.id_token(provider, &g, id), "token_type": "Bearer"})
                .to_string(),
        )
    }

    /// Apple's client secret is an ES256 JWT from the team's .p8 key.
    fn apple_secret_ok(&self, jwt: &str) -> bool {
        let parts: Vec<&str> = jwt.split('.').collect();
        let claims: Value = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &self.apple_public)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &B64.decode(parts[2]).unwrap(),
            )
            .is_ok()
            && claims["iss"] == "TEAM123456"
            && claims["sub"] == APPLE_ID
            && claims["aud"] == format!("{}/apple", self.base)
    }

    fn jwks(&self) -> String {
        let c = PublicKeyComponents::<Vec<u8>>::from(self.rsa.public_key());
        json!({"keys": [{"kty": "RSA", "kid": "k1", "alg": "RS256", "use": "sig",
                         "n": B64.encode(&c.n), "e": B64.encode(&c.e)}]})
        .to_string()
    }
}

async fn fixture(pool: PgPool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let apple_pkcs8 = EcdsaKeyPair::generate_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &SystemRandom::new(),
    )
    .unwrap();
    let apple = EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        apple_pkcs8.as_ref(),
    )
    .unwrap();
    let fx = Fixture {
        pool,
        revoke_link: Default::default(),
        base: base.clone(),
        grants: Default::default(),
        rsa: Arc::new(RsaKeyPair::generate(KeySize::Rsa2048).unwrap()),
        apple_public: apple.public_key().as_ref().to_vec(),
        forged_nonce: Default::default(),
    };
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----\\n{}\\n-----END PRIVATE KEY-----",
        base64::engine::general_purpose::STANDARD.encode(apple_pkcs8.as_ref())
    );
    // Safety: this test binary sets these once, before the app reads them.
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("OAUTH_FIXTURE_BASE", &base);
        std::env::set_var("GOOGLE_OAUTH_CLIENT_ID", GOOGLE_ID);
        std::env::set_var("GOOGLE_OAUTH_CLIENT_SECRET", GOOGLE_SECRET);
        std::env::set_var("APPLE_OAUTH_CLIENT_ID", APPLE_ID);
        std::env::set_var("APPLE_OAUTH_TEAM_ID", "TEAM123456");
        std::env::set_var("APPLE_OAUTH_KEY_ID", "KEY1234567");
        std::env::set_var("APPLE_OAUTH_PRIVATE_KEY", pem);
    }
    let router = Router::new()
        .route(
            "/{provider}/token",
            post(
                |State(fx): State<Fixture>,
                 axum::extract::Path(p): axum::extract::Path<String>,
                 body: axum::body::Bytes| async move {
                    fx.token(&p, &form(&body))
                        .map(|b| ([("content-type", "application/json")], b))
                },
            ),
        )
        .route(
            "/{provider}/jwks",
            get(|State(fx): State<Fixture>| async move {
                ([("content-type", "application/json")], fx.jwks())
            }),
        )
        .with_state(fx.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    fx
}

struct Done {
    location: String,
    cookie: Option<String>,
}

async fn raw(app: &Router, req: Request<Body>) -> Done {
    let r = app.clone().oneshot(req).await.unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    Done {
        location: r.headers()["location"]
            .to_str()
            .unwrap()
            .trim_start_matches(ORIGIN)
            .to_string(),
        cookie: r
            .headers()
            .get("set-cookie")
            .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string()),
    }
}

/// Starts a sign-in, lets the "provider" approve it, and follows the return.
#[allow(clippy::too_many_arguments)]
async fn sign_in(
    app: &Router,
    fx: &Fixture,
    provider: &str,
    sub: &str,
    email: Option<&str>,
    verified: Value,
    linking: Option<&User>,
    signup: bool,
) -> Done {
    let body = match linking {
        Some(u) => {
            // Isolate OAuth proof consumption here; real authenticator proof
            // issuance and session binding are covered in passkeys.rs.
            let token = audeniq_core::auth::random_token();
            let cookie_token = u.cookie.split_once('=').unwrap().1;
            sqlx::query("INSERT INTO identity.passkey_proofs(token_hash,session_hash,user_id,expires_at) VALUES($1,$2,$3,now()+interval '5 minutes')")
                .bind(audeniq_core::auth::hash_token(&token))
                .bind(audeniq_core::auth::hash_token(cookie_token)).bind(u.user).execute(&fx.pool).await.unwrap();
            json!({"mode":"link","reauth_token":token})
        }
        None => {
            json!({"return_to": "/releases", "signup": signup, "terms_version": "AUD-TERMS 2026.10.1", "privacy_notice_version": "AUD-PRIVACY 1.0-draft"})
        }
    };
    let (s, v) = send(
        app,
        "POST",
        &format!("/api/auth/oauth/{provider}/start"),
        body,
        linking,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revoke = *fx.revoke_link.lock().unwrap();
    if revoke {
        let u = linking.unwrap();
        sqlx::query("UPDATE identity.sessions SET revoked_at=now() WHERE token_hash=$1")
            .bind(audeniq_core::auth::hash_token(
                u.cookie.split_once('=').unwrap().1,
            ))
            .execute(&fx.pool)
            .await
            .unwrap();
    }
    let url = url::Url::parse(v["url"].as_str().unwrap()).unwrap();
    assert!(
        url.as_str()
            .starts_with(&format!("{}/{provider}/authorize", fx.base))
    );
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(
        q["redirect_uri"],
        format!("{ORIGIN}/api/auth/oauth/{provider}/callback")
    );
    assert_eq!(q["code_challenge_method"], "S256");
    let code = Uuid::new_v4().to_string();
    fx.grants.lock().unwrap().insert(
        code.clone(),
        Grant {
            challenge: q["code_challenge"].clone(),
            nonce: q["nonce"].clone(),
            sub: sub.into(),
            email: email.map(Into::into),
            verified,
        },
    );
    callback(app, provider, &[("code", &code), ("state", &q["state"])]).await
}

async fn callback(app: &Router, provider: &str, params: &[(&str, &str)]) -> Done {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    let req = if provider == "apple" {
        // Apple: a cross-site form POST from appleid.apple.com.
        Request::builder()
            .method("POST")
            .uri("/api/auth/oauth/apple/callback")
            .header("x-audeniq-service", SECRET)
            .header("origin", "https://appleid.apple.com")
            .header("sec-fetch-site", "cross-site")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(encoded))
            .unwrap()
    } else {
        Request::builder()
            .uri(format!("/api/auth/oauth/{provider}/callback?{encoded}"))
            .header("x-audeniq-service", SECRET)
            .body(Body::empty())
            .unwrap()
    };
    raw(app, req).await
}

async fn me(app: &Router, cookie: &str) -> Value {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("x-audeniq-service", SECRET)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()).unwrap()
}

#[sqlx::test]
async fn google_and_apple_sign_in(pool: PgPool) {
    let fx = fixture(pool.clone()).await;
    let (app, _) = app(pool.clone()).await;
    let (_, v) = send(&app, "GET", "/api/auth/oauth/providers", Value::Null, None).await;
    assert_eq!(v, json!({"google": true, "apple": true}));

    // First Google sign-in: a new account for the verified email, then back
    // to where the user was going, signed in.
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-100",
        Some("New.Artist@Gmail.com"),
        json!(true),
        None,
        true,
    )
    .await;
    assert_eq!(d.location, "/releases");
    let cookie = d.cookie.expect("session cookie");
    let m = me(&app, &cookie).await;
    assert_eq!(m["email"], "new.artist@gmail.com");
    let user = m["user_id"].as_str().unwrap().to_string();
    // The same Google account signs in to the same user.
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-100",
        Some("new.artist@gmail.com"),
        json!(true),
        None,
        false,
    )
    .await;
    assert_eq!(me(&app, &d.cookie.unwrap()).await["user_id"], user);
    // The connected email is stored sealed; there is no usable password.
    let stored: String = sqlx::query_scalar(
        "SELECT row_to_json(i)::text FROM identity.oauth_identities i WHERE subject='g-100'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!stored.contains("gmail"), "{stored}");
    let (s, _) = send(
        &app,
        "POST",
        "/api/auth/login",
        json!({"email": "new.artist@gmail.com", "password": "Some-guessed-password-1"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // A first sign-in from the login page (terms not agreed) goes to sign-up.
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-150",
        Some("later@gmail.com"),
        json!(true),
        None,
        false,
    )
    .await;
    assert_eq!(d.location, "/signup?oauth_error=OAUTH_SIGNUP_REQUIRED");
    assert!(d.cookie.is_none());

    // Refused: an unverified email, an email that already has an account
    // (no silent linking), a replayed state, a forged nonce, a cancel.
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-200",
        Some("x@example.test"),
        json!(false),
        None,
        true,
    )
    .await;
    assert_eq!(d.location, "/login?oauth_error=OAUTH_EMAIL_REQUIRED");
    assert!(d.cookie.is_none());
    let owner = user_with_email(&app, &pool).await;
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-300",
        Some(&owner.1),
        json!(true),
        None,
        true,
    )
    .await;
    assert_eq!(d.location, "/login?oauth_error=OAUTH_EMAIL_IN_USE");
    let (s, v) = send(
        &app,
        "POST",
        "/api/auth/oauth/google/start",
        json!({}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let state: String = url::Url::parse(v["url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let d = callback(
        &app,
        "google",
        &[("error", "access_denied"), ("state", &state)],
    )
    .await;
    assert_eq!(d.location, "/login?oauth_error=OAUTH_CANCELLED");
    let d = callback(&app, "google", &[("code", "x"), ("state", &state)]).await;
    assert_eq!(
        d.location, "/login?oauth_error=OAUTH_STATE_INVALID",
        "state is single use"
    );
    *fx.forged_nonce.lock().unwrap() = Some("other".into());
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-100",
        Some("new.artist@gmail.com"),
        json!(true),
        None,
        false,
    )
    .await;
    assert_eq!(d.location, "/login?oauth_error=OAUTH_TOKEN_INVALID");
    *fx.forged_nonce.lock().unwrap() = None;

    let (_, denied) = send(
        &app,
        "POST",
        "/api/auth/oauth/google/start",
        json!({"mode":"link"}),
        Some(&owner.0),
    )
    .await;
    assert_eq!(denied["error"]["code"], "PASSKEY_STEP_UP_REQUIRED");

    // A password account connects Google from its settings, then signs in with it.
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-300",
        Some(&owner.1),
        json!(true),
        Some(&owner.0),
        false,
    )
    .await;
    assert_eq!(d.location, "/profile?oauth_linked=google");
    let d = sign_in(&app, &fx, "google", "g-300", None, json!(true), None, false).await;
    assert_eq!(
        me(&app, &d.cookie.unwrap()).await["user_id"],
        owner.0.user.to_string()
    );
    // A Google account already connected elsewhere cannot be connected again.
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-100",
        None,
        json!(true),
        Some(&owner.0),
        false,
    )
    .await;
    assert_eq!(d.location, "/profile?oauth_error=OAUTH_ALREADY_LINKED");

    // Apple: form POST return, string "true" email_verified, JWT client secret.
    let d = sign_in(
        &app,
        &fx,
        "apple",
        "a-001.xyz",
        Some("abc@privaterelay.appleid.com"),
        json!("true"),
        None,
        true,
    )
    .await;
    assert_eq!(d.location, "/releases");
    assert_eq!(
        me(&app, &d.cookie.unwrap()).await["email"],
        "abc@privaterelay.appleid.com"
    );

    // Connected accounts: the last way to sign in cannot be removed.
    let (_, list) = send(
        &app,
        "GET",
        "/api/auth/oauth/identities",
        Value::Null,
        Some(&owner.0),
    )
    .await;
    assert_eq!(list["has_password"], true);
    assert_eq!(list["items"][0]["provider"], "google");
    assert_eq!(list["items"][0]["email"], owner.1);
    let id = list["items"][0]["id"].as_str().unwrap().to_string();
    let (s, v) = send(
        &app,
        "DELETE",
        &format!("/api/auth/oauth/identities/{id}"),
        Value::Null,
        Some(&owner.0),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["items"], json!([]));
    let oauth_only: Uuid = user.parse().unwrap();
    let only_id: Uuid =
        sqlx::query_scalar("SELECT id FROM identity.oauth_identities WHERE user_id=$1")
            .bind(oauth_only)
            .fetch_one(&pool)
            .await
            .unwrap();
    let session = User {
        user: oauth_only,
        cookie: cookie.clone(),
        csrf: csrf(&app, &cookie).await,
        ..owner.0.clone()
    };
    let (_, v) = send(
        &app,
        "DELETE",
        &format!("/api/auth/oauth/identities/{only_id}"),
        Value::Null,
        Some(&session),
    )
    .await;
    assert_eq!(v["error"]["code"], "OAUTH_LAST_SIGN_IN_METHOD");

    let lost = user_with_email(&app, &pool).await;
    *fx.revoke_link.lock().unwrap() = true;
    let d = sign_in(
        &app,
        &fx,
        "google",
        "g-revoked-link",
        Some(&lost.1),
        json!(true),
        Some(&lost.0),
        false,
    )
    .await;
    *fx.revoke_link.lock().unwrap() = false;
    assert_eq!(d.location, "/profile?oauth_error=OAUTH_FAILED");
    let linked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.oauth_identities WHERE user_id=$1")
            .bind(lost.0.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        linked, 0,
        "revoked initiating sessions cannot finish linking"
    );

    let reasons: Vec<String> = sqlx::query_scalar(
        "SELECT action||':'||reason_code FROM operations.audit_events WHERE action IN ('auth.register','auth.login','auth.oauth.linked','auth.oauth.unlinked')",
    )
    .fetch_all(&pool).await.unwrap();
    for r in [
        "auth.register:GOOGLE",
        "auth.register:APPLE",
        "auth.login:GOOGLE",
        "auth.login:APPLE",
        "auth.oauth.linked:GOOGLE",
        "auth.oauth.unlinked:GOOGLE",
    ] {
        assert!(reasons.iter().any(|x| x == r), "{r}: {reasons:?}");
    }
}

async fn user_with_email(app: &Router, pool: &PgPool) -> (User, String) {
    let u = user(app).await;
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let email = audeniq_core::pii::user_email(&keys, pool, u.user)
        .await
        .unwrap();
    (u, email)
}

async fn csrf(app: &Router, cookie: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/csrf")
                .header("x-audeniq-service", SECRET)
                .header("origin", ORIGIN)
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let v: Value =
        serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap())
            .unwrap();
    v["csrf_token"].as_str().unwrap().to_string()
}
