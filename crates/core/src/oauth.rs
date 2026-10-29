//! Google / Apple sign-in: OpenID Connect authorization code flow.
//!
//! 1. `POST /api/auth/oauth/{provider}/start` stores a one-time state (hashed),
//!    a nonce (hashed) and a PKCE verifier, and returns the provider URL.
//! 2. The provider sends the browser back to
//!    `{APP_ORIGIN}/api/auth/oauth/{provider}/callback` (Google: GET, Apple:
//!    form POST). The server exchanges the code at the token endpoint (with
//!    the client secret; Apple's is an ES256 JWT signed with the .p8 key),
//!    verifies the ID token signature (provider JWKS, cached), issuer,
//!    audience, expiry and nonce, then signs in and redirects to the studio.
//!
//! Accounts are found by the provider's user id (`sub`). A first sign-in with
//! a verified email that has no account creates one. An email that already
//! has an account is not linked automatically (pre-hijacking and takeover
//! protection): sign in with the password and connect the provider in the
//! account settings. A provider is on when its settings are complete.
use crate::config::env_value as env;
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    operations,
    payout_keys::Field,
};
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;

const STATE_SECONDS: i32 = 600;
/// Password hash of an account created by Google/Apple: no password matches it.
/// The owner can set one with "find password" (SMS reset).
pub const NO_PASSWORD: &str = "!oauth";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Google,
    Apple,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "google" => Some(Self::Google),
            "apple" => Some(Self::Apple),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Self::Google => "GOOGLE",
            Self::Apple => "APPLE",
        }
    }
}

enum Secret {
    Static(String),
    /// Apple: the client secret is a short-lived JWT signed with the .p8 key.
    Apple {
        team: String,
        key_id: String,
        pkcs8: Vec<u8>,
    },
}

pub struct Provider {
    kind: Kind,
    client_id: String,
    secret: Secret,
    authorize: String,
    token: String,
    jwks: String,
    issuers: Vec<String>,
}

/// Development/test only: one loopback HTTP server stands in for both
/// providers at `{base}/{provider}/authorize|token|jwks`. Ignored in production.
fn fixture() -> Option<String> {
    let base = env("OAUTH_FIXTURE_BASE")?;
    let local = crate::external_http::local_http_enabled(true);
    crate::external_http::validate_url(&base, local)
        .ok()
        .map(|_| base.trim_end_matches('/').to_owned())
}

/// An Apple .p8 key (PEM, "\n" escapes allowed) as PKCS#8 DER.
fn pem_pkcs8(pem: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::STANDARD;
    let pem = pem.replace("\\n", "\n");
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .flat_map(|l| l.trim().chars())
        .collect();
    let der = STANDARD.decode(body).ok()?;
    aws_lc_rs::signature::EcdsaKeyPair::from_pkcs8(
        &aws_lc_rs::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &der,
    )
    .ok()?;
    Some(der)
}

/// The provider when its settings are complete (and valid), else `None`.
pub fn provider(kind: Kind) -> Option<Provider> {
    let fx = fixture();
    let ends = |real: [&str; 4]| -> ([String; 3], Vec<String>) {
        match &fx {
            Some(base) => {
                let p = format!("{base}/{}", kind.name());
                (
                    [
                        format!("{p}/authorize"),
                        format!("{p}/token"),
                        format!("{p}/jwks"),
                    ],
                    vec![p],
                )
            }
            None => (
                [real[0].into(), real[1].into(), real[2].into()],
                real[3].split(',').map(str::to_owned).collect(),
            ),
        }
    };
    match kind {
        Kind::Google => {
            let ([authorize, token, jwks], issuers) = ends([
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://oauth2.googleapis.com/token",
                "https://www.googleapis.com/oauth2/v3/certs",
                "https://accounts.google.com,accounts.google.com",
            ]);
            Some(Provider {
                kind,
                client_id: env("GOOGLE_OAUTH_CLIENT_ID")?,
                secret: Secret::Static(env("GOOGLE_OAUTH_CLIENT_SECRET")?),
                authorize,
                token,
                jwks,
                issuers,
            })
        }
        Kind::Apple => {
            let ([authorize, token, jwks], issuers) = ends([
                "https://appleid.apple.com/auth/authorize",
                "https://appleid.apple.com/auth/token",
                "https://appleid.apple.com/auth/keys",
                "https://appleid.apple.com",
            ]);
            Some(Provider {
                kind,
                client_id: env("APPLE_OAUTH_CLIENT_ID")?,
                secret: Secret::Apple {
                    team: env("APPLE_OAUTH_TEAM_ID")?,
                    key_id: env("APPLE_OAUTH_KEY_ID")?,
                    pkcs8: pem_pkcs8(&env("APPLE_OAUTH_PRIVATE_KEY")?)?,
                },
                authorize,
                token,
                jwks,
                issuers,
            })
        }
    }
}

fn redirect_uri(s: &AppState, kind: Kind) -> String {
    format!(
        "{}/api/auth/oauth/{}/callback",
        s.config.origin,
        kind.name()
    )
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        crate::external_http::client_builder(fixture().is_some())
            .timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP client")
    })
}

fn jwt_part<T: serde::de::DeserializeOwned>(part: &str) -> Option<T> {
    serde_json::from_slice(&B64.decode(part).ok()?).ok()
}

fn apple_client_secret(p: &Provider, team: &str, key_id: &str, pkcs8: &[u8]) -> Result<String> {
    use aws_lc_rs::{rand::SystemRandom, signature};
    let now = chrono::Utc::now().timestamp();
    let header = B64.encode(json!({"alg": "ES256", "kid": key_id}).to_string());
    let claims = B64.encode(
        json!({"iss": team, "iat": now, "exp": now + 300, "aud": p.issuers[0], "sub": p.client_id})
            .to_string(),
    );
    let input = format!("{header}.{claims}");
    let key =
        signature::EcdsaKeyPair::from_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8)
            .map_err(|_| Error::Internal)?;
    let sig = key
        .sign(&SystemRandom::new(), input.as_bytes())
        .map_err(|_| Error::Internal)?;
    Ok(format!("{input}.{}", B64.encode(sig.as_ref())))
}

// ---------------------------------------------------------------------------
// ID token verification
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

type KeyCache = Mutex<HashMap<String, (Instant, Vec<Jwk>)>>;

fn cache() -> &'static KeyCache {
    static C: OnceLock<KeyCache> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// The provider's signing keys: cached for an hour, refetched (at most once a
/// minute) when a token names a key we have not seen (key rotation).
async fn key(p: &Provider, kid: &str) -> Result<Jwk> {
    let find = |keys: &[Jwk]| keys.iter().find(|k| k.kid.as_deref() == Some(kid)).cloned();
    let cached = cache()
        .lock()
        .map_err(|_| Error::Internal)?
        .get(&p.jwks)
        .cloned();
    if let Some((at, keys)) = &cached {
        if let Some(k) = find(keys) {
            if at.elapsed() < Duration::from_secs(3600) {
                return Ok(k);
            }
        } else if at.elapsed() < Duration::from_secs(60) {
            return Err(Error::Unauthorized);
        }
    }
    #[derive(Deserialize)]
    struct Set {
        keys: Vec<Jwk>,
    }
    let set: Set = http()
        .get(&p.jwks)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| Error::PolicyGate("OAUTH_PROVIDER_UNAVAILABLE"))?
        .json()
        .await
        .map_err(|_| Error::PolicyGate("OAUTH_PROVIDER_UNAVAILABLE"))?;
    let k = find(&set.keys);
    cache()
        .lock()
        .map_err(|_| Error::Internal)?
        .insert(p.jwks.clone(), (Instant::now(), set.keys));
    k.ok_or(Error::Unauthorized)
}

fn signature_ok(alg: &str, jwk: &Jwk, input: &[u8], sig: &[u8]) -> bool {
    use aws_lc_rs::signature;
    match (alg, jwk.kty.as_str()) {
        ("RS256", "RSA") => {
            let (Some(n), Some(e)) = (
                jwk.n.as_deref().and_then(|v| B64.decode(v).ok()),
                jwk.e.as_deref().and_then(|v| B64.decode(v).ok()),
            ) else {
                return false;
            };
            signature::RsaPublicKeyComponents { n: &n, e: &e }
                .verify(&signature::RSA_PKCS1_2048_8192_SHA256, input, sig)
                .is_ok()
        }
        ("ES256", "EC") if jwk.crv.as_deref() == Some("P-256") => {
            let (Some(x), Some(y)) = (
                jwk.x.as_deref().and_then(|v| B64.decode(v).ok()),
                jwk.y.as_deref().and_then(|v| B64.decode(v).ok()),
            ) else {
                return false;
            };
            let mut point = vec![4u8];
            point.extend_from_slice(&x);
            point.extend_from_slice(&y);
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point)
                .verify(input, sig)
                .is_ok()
        }
        _ => false,
    }
}

#[derive(Deserialize)]
pub struct Claims {
    pub iss: String,
    pub aud: Value,
    pub sub: String,
    pub exp: i64,
    pub iat: Option<i64>,
    pub nonce: Option<String>,
    pub email: Option<String>,
    /// Google: bool. Apple: "true"/"false" or bool.
    pub email_verified: Option<Value>,
}

impl Claims {
    pub fn verified_email(&self) -> Option<String> {
        let verified = matches!(&self.email_verified, Some(Value::Bool(true)))
            || matches!(&self.email_verified, Some(Value::String(v)) if v == "true");
        self.email
            .as_deref()
            .map(|e| e.trim().to_lowercase())
            .filter(|e| verified && e.len() <= 254 && e.contains('@'))
    }
}

async fn verify_id_token(p: &Provider, token: &str, nonce_hash: &[u8]) -> Result<Claims> {
    #[derive(Deserialize)]
    struct Head {
        alg: String,
        kid: Option<String>,
    }
    let parts: Vec<&str> = token.split('.').collect();
    let [h, c, sig] = parts[..] else {
        return Err(Error::Unauthorized);
    };
    let head: Head = jwt_part(h).ok_or(Error::Unauthorized)?;
    let jwk = key(p, head.kid.as_deref().unwrap_or("")).await?;
    let sig = B64.decode(sig).map_err(|_| Error::Unauthorized)?;
    if !signature_ok(&head.alg, &jwk, format!("{h}.{c}").as_bytes(), &sig) {
        return Err(Error::Unauthorized);
    }
    let claims: Claims = jwt_part(c).ok_or(Error::Unauthorized)?;
    let now = chrono::Utc::now().timestamp();
    let audience = match &claims.aud {
        Value::String(a) => a == &p.client_id,
        Value::Array(a) => a.iter().any(|v| v.as_str() == Some(&p.client_id)),
        _ => false,
    };
    let ok = p.issuers.contains(&claims.iss)
        && audience
        && claims.exp > now - 60
        && claims.iat.is_none_or(|t| t < now + 300)
        && !claims.sub.is_empty()
        && claims.sub.len() <= 255
        && claims
            .nonce
            .as_deref()
            .is_some_and(|n| bool::from(Sha256::digest(n.as_bytes()).ct_eq(nonce_hash)));
    if ok {
        Ok(claims)
    } else {
        Err(Error::Unauthorized)
    }
}

async fn exchange(s: &AppState, p: &Provider, code: &str, verifier: &str) -> Result<String> {
    let secret = match &p.secret {
        Secret::Static(v) => v.clone(),
        Secret::Apple {
            team,
            key_id,
            pkcs8,
        } => apple_client_secret(p, team, key_id, pkcs8)?,
    };
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", code)
        .append_pair("redirect_uri", &redirect_uri(s, p.kind))
        .append_pair("client_id", &p.client_id)
        .append_pair("client_secret", &secret)
        .append_pair("code_verifier", verifier)
        .finish();
    #[derive(Deserialize)]
    struct Tokens {
        id_token: String,
    }
    let r = http()
        .post(&p.token)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ACCEPT, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| Error::PolicyGate("OAUTH_PROVIDER_UNAVAILABLE"))?;
    if !r.status().is_success() {
        return Err(Error::PolicyGate("OAUTH_CODE_REJECTED"));
    }
    let t: Tokens = r
        .json()
        .await
        .map_err(|_| Error::PolicyGate("OAUTH_CODE_REJECTED"))?;
    Ok(t.id_token)
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

pub fn providers() -> Value {
    json!({
        "google": provider(Kind::Google).is_some(),
        "apple": provider(Kind::Apple).is_some(),
    })
}

fn kind(name: &str) -> Result<Kind> {
    Kind::parse(name).ok_or(Error::NotFound)
}

/// A path inside the studio, else "/".
fn safe_return(to: Option<&str>) -> String {
    to.filter(|p| {
        p.len() <= 200
            && p.starts_with('/')
            && !p.starts_with("//")
            && !p.contains('\\')
            && !p.chars().any(char::is_control)
    })
    .unwrap_or("/")
    .to_owned()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartInput {
    /// "login" (default) or "link" (connect to the signed-in account).
    pub mode: Option<String>,
    pub reauth_token: Option<String>,
    pub return_to: Option<String>,
    /// Started from the sign-up page after agreeing to the required terms:
    /// a first sign-in may create the account. Otherwise it is sent there.
    pub signup: Option<bool>,
    /// With `signup`: the terms (required) and privacy notice versions agreed.
    pub terms_version: Option<String>,
    pub privacy_notice_version: Option<String>,
}

pub async fn start(s: &AppState, h: &HeaderMap, name: &str, i: StartInput) -> Result<Value> {
    auth::origin(h, &s.config)?;
    let kind = kind(name)?;
    let p = provider(kind).ok_or(Error::PolicyGate("OAUTH_PROVIDER_NOT_CONFIGURED"))?;
    let mut link_session = None;
    let user = match i.mode.as_deref() {
        None | Some("login") => None,
        Some("link") => {
            let a = auth::actor(&s.pool, h, &s.config, true).await?;
            let mut tx = s.pool.begin().await?;
            crate::passkey::consume_proof(&mut tx, &a, i.reauth_token.as_deref().unwrap_or(""))
                .await?;
            tx.commit().await?;
            link_session = Some(a.session_hash);
            Some(a.user)
        }
        Some(_) => return Err(Error::Invalid),
    };
    let (src, factor) = match auth::client_ip_key(h) {
        Some(ip) => (ip, 1),
        None => ("unknown".into(), auth::UNKNOWN_SOURCE_FACTOR),
    };
    auth::rate(&s.pool, &format!("oauth-start:src:{src}"), 60 * factor).await?;
    // Sign-up needs the agreed terms version (recorded when the account is made).
    let signup = user.is_none() && i.signup == Some(true);
    if signup
        && !i.terms_version.as_deref().is_some_and(|v| {
            let v = v.trim();
            !v.is_empty()
                && v.len() <= 60
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b" ._-".contains(&b))
        })
    {
        return Err(Error::InvalidCode("TERMS_ACCEPTANCE_REQUIRED"));
    }
    let (state, nonce, verifier) = (
        auth::random_token(),
        auth::random_token(),
        auth::random_token(),
    );
    sqlx::query("DELETE FROM identity.oauth_states WHERE expires_at < now() - interval '1 hour'")
        .execute(&s.pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.oauth_states(id,state_hash,provider,purpose,user_id,nonce_hash,code_verifier,return_to,allow_signup,expires_at,terms_version,privacy_notice_version,session_hash)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,now()+make_interval(secs=>$10),$11,$12,$13)",
    )
    .bind(Uuid::new_v4())
    .bind(auth::hash_token(&state))
    .bind(kind.name())
    .bind(if user.is_some() { "LINK" } else { "LOGIN" })
    .bind(user)
    .bind(Sha256::digest(nonce.as_bytes()).to_vec())
    .bind(&verifier)
    .bind(safe_return(i.return_to.as_deref()))
    .bind(signup)
    .bind(f64::from(STATE_SECONDS))
    .bind(i.terms_version.as_deref().map(str::trim))
    .bind(i.privacy_notice_version.as_deref().map(str::trim))
    .bind(link_session)
    .execute(&s.pool)
    .await?;
    let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
    let redirect = redirect_uri(s, kind);
    let mut params = vec![
        ("client_id", p.client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        ("state", state.as_str()),
        ("nonce", nonce.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    match kind {
        Kind::Google => params.extend([("scope", "openid email"), ("prompt", "select_account")]),
        // Apple sends the email only with form_post.
        Kind::Apple => params.extend([("scope", "email"), ("response_mode", "form_post")]),
    }
    let url = url::Url::parse_with_params(&p.authorize, &params).map_err(|_| Error::Internal)?;
    Ok(json!({"url": url.as_str()}))
}

/// What the provider sends back (query for Google, form body for Apple).
#[derive(Deserialize, Default)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

struct Pending {
    session_hash: Option<Vec<u8>>,
    purpose: String,
    user: Option<Uuid>,
    nonce_hash: Vec<u8>,
    verifier: String,
    return_to: String,
    allow_signup: bool,
    terms_version: Option<String>,
    privacy_notice_version: Option<String>,
}

fn redirect(s: &AppState, path: &str, cookies: Option<HeaderMap>) -> Response {
    let mut r = StatusCode::SEE_OTHER.into_response();
    let headers = r.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&format!("{}{path}", s.config.origin)) {
        headers.insert(header::LOCATION, v);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if let Some(c) = cookies {
        for v in c.get_all(header::SET_COOKIE) {
            headers.append(header::SET_COOKIE, v.clone());
        }
    }
    r
}

fn error_code(e: &Error) -> &'static str {
    match e {
        Error::PolicyGate(c) | Error::InvalidCode(c) => c,
        Error::RateLimited => "RATE_LIMITED",
        _ => "OAUTH_FAILED",
    }
}

/// The provider's redirect back. Always answers with a redirect to the studio:
/// signed in, or `/login?oauth_error=CODE` (`/profile?...` when connecting).
pub async fn callback(s: &AppState, h: &HeaderMap, name: &str, p: CallbackParams) -> Response {
    let Some(kind) = Kind::parse(name) else {
        return redirect(s, "/login?oauth_error=OAUTH_FAILED", None);
    };
    let pending = match consume(s, kind, p.state.as_deref()).await {
        Ok(Some(v)) => v,
        Ok(None) => return redirect(s, "/login?oauth_error=OAUTH_STATE_INVALID", None),
        Err(e) => return redirect(s, &format!("/login?oauth_error={}", error_code(&e)), None),
    };
    let link = pending.purpose == "LINK";
    let fail_to = if link { "/profile" } else { "/login" };
    let outcome = match (p.error.as_deref(), p.code.as_deref()) {
        (Some(_), _) | (None, None) => Err(Error::PolicyGate("OAUTH_CANCELLED")),
        (None, Some(code)) => complete(s, h, kind, &pending, code).await,
    };
    match outcome {
        Ok(cookies) => {
            let to = if link {
                format!("/profile?oauth_linked={}", kind.name())
            } else {
                pending.return_to.clone()
            };
            redirect(s, &to, cookies)
        }
        Err(e) => {
            // No account yet: agree to the terms on the sign-up page first.
            let fail_to = if matches!(e, Error::PolicyGate("OAUTH_SIGNUP_REQUIRED")) {
                "/signup"
            } else {
                fail_to
            };
            if !matches!(e, Error::PolicyGate("OAUTH_CANCELLED")) {
                tracing::info!(
                    provider = kind.name(),
                    code = error_code(&e),
                    "oauth sign-in refused"
                );
            }
            redirect(
                s,
                &format!("{fail_to}?oauth_error={}", error_code(&e)),
                None,
            )
        }
    }
}

async fn consume(s: &AppState, kind: Kind, state: Option<&str>) -> Result<Option<Pending>> {
    let Some(state) = state.filter(|v| v.len() == 64) else {
        return Ok(None);
    };
    let row = sqlx::query(
        "UPDATE identity.oauth_states SET consumed_at=now()
          WHERE state_hash=$1 AND provider=$2 AND consumed_at IS NULL AND expires_at>now()
          RETURNING purpose,user_id,nonce_hash,code_verifier,return_to,allow_signup,terms_version,privacy_notice_version,session_hash",
    )
    .bind(auth::hash_token(state))
    .bind(kind.name())
    .fetch_optional(&s.pool)
    .await?;
    Ok(row.map(|r| Pending {
        session_hash: r.get("session_hash"),
        purpose: r.get("purpose"),
        user: r.get("user_id"),
        nonce_hash: r.get("nonce_hash"),
        verifier: r.get("code_verifier"),
        return_to: r.get("return_to"),
        allow_signup: r.get("allow_signup"),
        terms_version: r.get("terms_version"),
        privacy_notice_version: r.get("privacy_notice_version"),
    }))
}

async fn complete(
    s: &AppState,
    h: &HeaderMap,
    kind: Kind,
    pending: &Pending,
    code: &str,
) -> Result<Option<HeaderMap>> {
    if code.is_empty() || code.len() > 2048 {
        return Err(Error::PolicyGate("OAUTH_CODE_REJECTED"));
    }
    let p = provider(kind).ok_or(Error::PolicyGate("OAUTH_PROVIDER_NOT_CONFIGURED"))?;
    let id_token = exchange(s, &p, code, &pending.verifier).await?;
    let claims = verify_id_token(&p, &id_token, &pending.nonce_hash)
        .await
        .map_err(|e| match e {
            Error::PolicyGate(_) => e,
            _ => Error::PolicyGate("OAUTH_TOKEN_INVALID"),
        })?;
    let email = claims.verified_email();
    let mut tx = s.pool.begin().await?;
    if let Some(user) = pending.user {
        sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .execute(&mut *tx)
            .await?;
        // A logout/revocation while the provider is open cancels the link.
        let live: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM identity.sessions WHERE token_hash=$1 AND user_id=$2 AND revoked_at IS NULL AND expires_at>now() FOR SHARE")
            .bind(&pending.session_hash).bind(user).fetch_optional(&mut *tx).await?;
        if live.is_none() {
            return Err(Error::Unauthorized);
        }
    }
    if pending.user.is_none() {
        // Login takes the same account-before-identity order as unlink.
        let _: Option<Uuid> = sqlx::query_scalar("SELECT u.id FROM identity.users u JOIN identity.oauth_identities i ON i.user_id=u.id WHERE i.provider=$1 AND i.subject=$2 FOR SHARE OF u")
            .bind(kind.name()).bind(&claims.sub).fetch_optional(&mut *tx).await?;
    }
    let existing: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT i.id,i.user_id,u.status FROM identity.oauth_identities i JOIN identity.users u ON u.id=i.user_id
          WHERE i.provider=$1 AND i.subject=$2 FOR UPDATE OF i",
    )
    .bind(kind.name())
    .bind(&claims.sub)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(user) = pending.user {
        // Connect to the signed-in account.
        if existing.is_some() {
            return Err(Error::PolicyGate("OAUTH_ALREADY_LINKED"));
        }
        let id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO identity.oauth_identities(id,provider,subject,user_id,email_cipher)
             SELECT $1,$2,$3,$4,$5 WHERE EXISTS(SELECT 1 FROM identity.users WHERE id=$4 AND status='ACTIVE')
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(kind.name())
        .bind(&claims.sub)
        .bind(user)
        .bind(seal(s, id, email.as_deref())?)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted != 1 {
            return Err(Error::PolicyGate("OAUTH_PROVIDER_LINKED"));
        }
        operations::audit(
            &mut tx,
            Some(user),
            None,
            Some(user),
            "auth.oauth.linked",
            kind.method(),
            auth::request_id(h),
        )
        .await?;
        tx.commit().await?;
        return Ok(None);
    }

    let user = match existing {
        Some((id, user, status)) => {
            if status != "ACTIVE" {
                return Err(Error::PolicyGate("ACCOUNT_NOT_ACTIVE"));
            }
            sqlx::query("UPDATE identity.oauth_identities SET last_used_at=now() WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            user
        }
        None => {
            // First sign-in: a new account for a verified email nobody uses
            // yet, only after the required terms were agreed on sign-up.
            let email = email.ok_or(Error::PolicyGate("OAUTH_EMAIL_REQUIRED"))?;
            if !pending.allow_signup {
                return Err(Error::PolicyGate("OAUTH_SIGNUP_REQUIRED"));
            }
            let (user, _, _) =
                auth::create_account(s, &mut tx, &email, NO_PASSWORD, kind.method(), h)
                    .await
                    .map_err(|e| match e {
                        Error::Conflict => Error::PolicyGate("OAUTH_EMAIL_IN_USE"),
                        e => e,
                    })?;
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO identity.oauth_identities(id,provider,subject,user_id,email_cipher,last_used_at)
                 VALUES($1,$2,$3,$4,$5,now())",
            )
            .bind(id)
            .bind(kind.name())
            .bind(&claims.sub)
            .bind(user)
            .bind(seal(s, id, Some(&email))?)
            .execute(&mut *tx)
            .await?;
            auth::record_acceptance(
                s,
                &mut tx,
                user,
                pending.terms_version.as_deref().unwrap_or(""),
                pending.privacy_notice_version.as_deref(),
                kind.method(),
            )
            .await?;
            user
        }
    };
    let (cookies, _) = auth::issue_session(s, tx, h, user, kind.method(), false).await?;
    Ok(Some(cookies))
}

fn seal(s: &AppState, id: Uuid, email: Option<&str>) -> Result<Option<Vec<u8>>> {
    email
        .map(|e| s.payout_keys.seal_field(Field::OAuthEmail, id, e))
        .transpose()
}

// ---------------------------------------------------------------------------
// Connected accounts (signed in)
// ---------------------------------------------------------------------------

pub async fn identities(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, false).await?;
    list_for(s, a.user).await
}

async fn list_for(s: &AppState, user: Uuid) -> Result<Value> {
    let rows = sqlx::query(
        "SELECT id,provider,email_cipher,created_at,last_used_at FROM identity.oauth_identities
          WHERE user_id=$1 ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(&s.pool)
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: Uuid = r.get("id");
        let email = match r.get::<Option<Vec<u8>>, _>("email_cipher") {
            Some(c) => Some(
                s.payout_keys
                    .open_field(Field::OAuthEmail, id, &c)?
                    .to_string(),
            ),
            None => None,
        };
        items.push(json!({
            "id": id,
            "provider": r.get::<String, _>("provider"),
            "email": email,
            "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            "last_used_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at"),
        }));
    }
    let has_password: bool =
        sqlx::query_scalar("SELECT password_hash LIKE '$argon2%' FROM identity.users WHERE id=$1")
            .bind(user)
            .fetch_one(&s.pool)
            .await?;
    Ok(json!({"items": items, "providers": providers(), "has_password": has_password}))
}

/// Disconnects a provider, unless it is the account's last way to sign in.
pub async fn unlink(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    // Read alternatives only after acquiring the lock, using a fresh snapshot.
    let others: bool = sqlx::query_scalar(
        "SELECT u.password_hash LIKE '$argon2%'
             OR EXISTS(SELECT 1 FROM identity.passkeys p WHERE p.user_id=u.id)
             OR EXISTS(SELECT 1 FROM identity.oauth_identities o WHERE o.user_id=u.id AND o.id<>$2)
           FROM identity.users u WHERE u.id=$1",
    )
    .bind(a.user)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    auth::lock_current_session(&mut tx, &a).await?;
    if !others {
        return Err(Error::PolicyGate("OAUTH_LAST_SIGN_IN_METHOD"));
    }
    let provider: Option<String> = sqlx::query_scalar(
        "DELETE FROM identity.oauth_identities WHERE id=$1 AND user_id=$2 RETURNING provider",
    )
    .bind(id)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?;
    let provider = provider.ok_or(Error::NotFound)?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "auth.oauth.unlinked",
        &provider.to_uppercase(),
        a.request,
    )
    .await?;
    tx.commit().await?;
    list_for(s, a.user).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_paths_stay_inside_the_studio() {
        assert_eq!(safe_return(Some("/releases?x=1")), "/releases?x=1");
        for bad in [
            "//evil.example",
            "https://evil.example",
            "/\\evil",
            "evil",
            "/a\nb",
        ] {
            assert_eq!(safe_return(Some(bad)), "/", "{bad}");
        }
        assert_eq!(safe_return(None), "/");
    }

    #[test]
    fn email_counts_only_when_the_provider_verified_it() {
        let c = |v: Value| Claims {
            iss: String::new(),
            aud: Value::Null,
            sub: "s".into(),
            exp: 0,
            iat: None,
            nonce: None,
            email: Some(" A@Example.com ".into()),
            email_verified: Some(v),
        };
        assert_eq!(
            c(json!(true)).verified_email().as_deref(),
            Some("a@example.com")
        );
        assert_eq!(
            c(json!("true")).verified_email().as_deref(),
            Some("a@example.com")
        );
        assert_eq!(c(json!(false)).verified_email(), None);
        assert_eq!(c(json!("false")).verified_email(), None);
    }

    #[test]
    fn apple_keys_are_read_from_pem_with_escaped_newlines() {
        use aws_lc_rs::{rand::SystemRandom, signature::EcdsaKeyPair};
        use base64::engine::general_purpose::STANDARD;
        let der = EcdsaKeyPair::generate_pkcs8(
            &aws_lc_rs::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &SystemRandom::new(),
        )
        .unwrap();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\\n{}\\n-----END PRIVATE KEY-----",
            STANDARD.encode(der.as_ref())
        );
        assert_eq!(pem_pkcs8(&pem).as_deref(), Some(der.as_ref()));
        assert!(pem_pkcs8("not a key").is_none());
    }
}
