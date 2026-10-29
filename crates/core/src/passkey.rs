//! Passkeys (WebAuthn): sign in with the device's passkey (fingerprint, face,
//! PIN) instead of the password.
//!
//! - The device keeps the private key; the server stores only the public key
//!   (X.509 SubjectPublicKeyInfo from `getPublicKey()`) and verifies each
//!   login signature over `authenticatorData || SHA-256(clientDataJSON)`.
//!   ES256, EdDSA and RS256 (aws-lc-rs, already built for TLS).
//! - Attestation is "none": the server does not trust a device model, so it
//!   reads the public key the browser reports. The credential id inside
//!   `authenticatorData` must match.
//! - Every ceremony uses a one-time challenge (stored hashed, 5 minutes) and
//!   checks the type, origin and RP ID hash. User verification is required,
//!   so a passkey login is multi-factor (possession + biometric/PIN): the
//!   session counts as second-factor verified, including for staff.
//! - Discoverable credentials (passkeys): the login options name no account,
//!   so they do not reveal whether an email exists. The email entered first
//!   must belong to the passkey that signs.
//! - Adding a passkey needs a fresh session-bound passkey proof or the emergency
//!   password, so a stolen session cannot plant its own login.
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    operations,
};
use axum::{Json, http::HeaderMap};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use subtle::ConstantTimeEq;
use uuid::Uuid;

// Static labels and durations only: no ceremony, account or credential data.
struct CeremonyTimer {
    ceremony: &'static str,
    start: std::time::Instant,
    overhead: std::time::Duration,
    last: std::time::Instant,
}
impl CeremonyTimer {
    fn new(ceremony: &'static str) -> Self {
        let now = std::time::Instant::now();
        Self {
            ceremony,
            start: now,
            overhead: std::time::Duration::ZERO,
            last: now,
        }
    }
    fn section(&mut self, section: &'static str) {
        let end = std::time::Instant::now();
        tracing::debug!(target: "passkey_timing", ceremony = self.ceremony, section,
            elapsed_us = end.duration_since(self.last).as_micros() as u64);
        // Exclude synchronous subscriber/formatting work from the next section
        // and handler total, including when the probe writes JSON to stdout.
        self.last = std::time::Instant::now();
        self.overhead += self.last.duration_since(end);
    }
}

impl Drop for CeremonyTimer {
    fn drop(&mut self) {
        tracing::debug!(target: "passkey_timing", ceremony = self.ceremony,
            section = "total", elapsed_us = self.start.elapsed().saturating_sub(self.overhead).as_micros() as u64);
    }
}

const CHALLENGE_SECONDS: i32 = 300;
const MAX_PER_USER: i64 = 10;
const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_BE: u8 = 0x08;
const FLAG_BS: u8 = 0x10;
const FLAG_AT: u8 = 0x40;
/// COSE algorithm ids, preferred first.
const ALGS: [i32; 3] = [-8, -7, -257];

/// Display hints only: attestation="none" does not authenticate the provider.
/// Curated from passkeydeveloper/passkey-authenticator-aaguids/aaguid.json,
/// checked 2026-10-29. No network requests or credential-specific identifiers.
/// Unknown and privacy-redacted (zero) AAGUIDs retain the supplied nickname.
const AUTHENTICATORS: &[(Uuid, &str)] = &[
    (
        Uuid::from_u128(0xbada_5566_a7aa_401f_bd96_4561_9a55_120d),
        "1Password",
    ),
    (
        Uuid::from_u128(0xd548_826e_79b4_db40_a3d8_1111_6f7e_8349),
        "Bitwarden",
    ),
    (
        Uuid::from_u128(0x5072_6f74_6f6e_5061_7373_5072_6f74_6f6e),
        "Proton Pass",
    ),
    (
        Uuid::from_u128(0x5311_26d6_e717_415c_9320_3d9a_a698_1239),
        "Dashlane",
    ),
    (
        Uuid::from_u128(0xea9b_8d66_4d01_1d21_3ce4_b6b4_8cb5_75d4),
        "Google Password Manager",
    ),
];

fn authenticator_name(aaguid: Uuid) -> Option<&'static str> {
    AUTHENTICATORS
        .iter()
        .find_map(|(id, name)| (*id == aaguid).then_some(*name))
}

fn host(origin: &str) -> Option<String> {
    url::Url::parse(origin)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
}

/// The RP ID: `PASSKEY_RP_ID` when it is the host of every site or a parent
/// domain of them (`audeniq.com` covers studio. and admin.), so one passkey
/// works on both; otherwise the studio host.
pub fn pick_rp_id(
    app_host: &str,
    admin_host: Option<&str>,
    configured: Option<&str>,
) -> Option<String> {
    let covers = |rp: &str, h: &str| h == rp || h.ends_with(&format!(".{rp}"));
    match configured
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty())
    {
        Some(rp) => {
            (covers(&rp, app_host) && admin_host.is_none_or(|a| covers(&rp, a))).then_some(rp)
        }
        None => Some(app_host.to_owned()),
    }
}

fn rp_id(s: &AppState) -> Result<String> {
    let app = host(&s.config.origin).ok_or(Error::Internal)?;
    let admin = crate::sites::admin_origin().and_then(host);
    let configured = std::env::var("PASSKEY_RP_ID").ok();
    pick_rp_id(&app, admin.as_deref(), configured.as_deref())
        .ok_or(Error::PolicyGate("PASSKEY_RP_ID_INVALID"))
}

/// Pages a passkey ceremony may come from: the studio, and the admin console.
fn origins(s: &AppState) -> Vec<&str> {
    let mut v = vec![s.config.origin.as_str()];
    v.extend(crate::sites::admin_origin());
    v
}

fn decode(v: &str, max: usize) -> Option<Vec<u8>> {
    if v.len() > max.div_ceil(3) * 4 {
        return None;
    }
    B64.decode(v)
        .ok()
        .filter(|b| !b.is_empty() && b.len() <= max)
}

async fn new_challenge<'e>(
    db: impl sqlx::PgExecutor<'e>,
    purpose: &str,
    user: Option<Uuid>,
    session: Option<&[u8]>,
) -> Result<(Uuid, String)> {
    let mut raw = [0u8; 32];
    {
        use rand::{TryRng, rngs::SysRng};
        SysRng
            .try_fill_bytes(&mut raw)
            .map_err(|_| Error::Internal)?;
    }
    let challenge = B64.encode(raw);
    let id = Uuid::new_v4();
    sqlx::query(
        // Bound cleanup work and skip rows in active ceremonies. Cleanup and insertion
        // share one round trip, without waiting on another cleanup worker.
        "WITH expired AS (
           SELECT id FROM identity.webauthn_challenges
            WHERE expires_at < statement_timestamp() - interval '1 hour'
            ORDER BY expires_at LIMIT 256 FOR UPDATE SKIP LOCKED
         ), removed AS (
           DELETE FROM identity.webauthn_challenges WHERE id IN (SELECT id FROM expired)
         )
         INSERT INTO identity.webauthn_challenges(id,challenge_hash,purpose,user_id,expires_at,session_hash)
         VALUES($1,$2,$3,$4,clock_timestamp()+make_interval(secs=>$5),$6)",
    )
    .bind(id)
    .bind(Sha256::digest(challenge.as_bytes()).to_vec())
    .bind(purpose)
    .bind(user)
    .bind(f64::from(CHALLENGE_SECONDS))
    .bind(session)
    .execute(db)
    .await?;
    Ok((id, challenge))
}

/// Marks the challenge used (once only) and returns its hash.
async fn consume(
    c: &mut sqlx::PgConnection,
    id: Uuid,
    purpose: &str,
    user: Option<Uuid>,
    session: Option<&[u8]>,
) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        "WITH candidate AS MATERIALIZED (
           SELECT id FROM identity.webauthn_challenges WHERE id=$1 FOR UPDATE SKIP LOCKED
         )
         UPDATE identity.webauthn_challenges SET consumed_at=clock_timestamp()
          WHERE id IN (SELECT id FROM candidate) AND id=$1 AND purpose=$2 AND user_id IS NOT DISTINCT FROM $3 AND session_hash IS NOT DISTINCT FROM $4
            AND consumed_at IS NULL AND expires_at>clock_timestamp() RETURNING challenge_hash",
    )
    .bind(id)
    .bind(purpose)
    .bind(user)
    .bind(session)
    .fetch_optional(c)
    .await?)
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(rename = "crossOrigin", default)]
    cross_origin: bool,
}

/// clientDataJSON: the ceremony type, our challenge, our exact origin.
pub(crate) fn client_data_ok(
    raw: &[u8],
    kind: &str,
    challenge_hash: &[u8],
    origins: &[&str],
) -> bool {
    serde_json::from_slice::<ClientData>(raw).is_ok_and(|c| {
        c.kind == kind
            && origins.contains(&c.origin.as_str())
            && !c.cross_origin
            && bool::from(Sha256::digest(c.challenge.as_bytes()).ct_eq(challenge_hash))
    })
}

pub struct AuthData<'a> {
    pub flags: u8,
    pub sign_count: u32,
    pub credential_id: Option<&'a [u8]>,
    pub aaguid: Option<Uuid>,
}

/// authenticatorData: RP ID hash, flags, counter, attested credential id.
pub fn auth_data<'a>(d: &'a [u8], rp_id: &str) -> Option<AuthData<'a>> {
    if d.len() < 37 || !bool::from(d[..32].ct_eq(&Sha256::digest(rp_id.as_bytes()))) {
        return None;
    }
    let flags = d[32];
    let sign_count = u32::from_be_bytes(d[33..37].try_into().ok()?);
    let credential_id = if flags & FLAG_AT != 0 {
        // aaguid (16) then a 2-byte credential id length.
        let len = usize::from(u16::from_be_bytes(d.get(53..55)?.try_into().ok()?));
        Some(d.get(55..55 + len)?)
    } else {
        None
    };
    Some(AuthData {
        flags,
        sign_count,
        credential_id,
        aaguid: if flags & FLAG_AT != 0 {
            Some(Uuid::from_slice(d.get(37..53)?).ok()?)
        } else {
            None
        },
    })
}

fn algorithm(alg: i32) -> Option<&'static dyn aws_lc_rs::signature::VerificationAlgorithm> {
    use aws_lc_rs::signature;
    match alg {
        -7 => Some(&signature::ECDSA_P256_SHA256_ASN1),
        -8 => Some(&signature::ED25519),
        -257 => Some(&signature::RSA_PKCS1_2048_8192_SHA256),
        _ => None,
    }
}

fn parsed_key(alg: i32, spki: &[u8]) -> Option<aws_lc_rs::signature::ParsedPublicKey> {
    aws_lc_rs::signature::ParsedPublicKey::new(algorithm(alg)?, spki).ok()
}

pub(crate) fn signature_ok(
    alg: i32,
    spki: &[u8],
    auth_data: &[u8],
    client_data: &[u8],
    sig: &[u8],
) -> bool {
    let Some(key) = parsed_key(alg, spki) else {
        return false;
    };
    let mut msg = Vec::with_capacity(auth_data.len() + 32);
    msg.extend_from_slice(auth_data);
    msg.extend_from_slice(&Sha256::digest(client_data));
    key.verify_sig(&msg, sig).is_ok()
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

fn source(h: &HeaderMap) -> (String, i32) {
    match auth::client_ip_key(h) {
        Some(ip) => (ip, 1),
        None => ("unknown".into(), auth::UNKNOWN_SOURCE_FACTOR),
    }
}

pub async fn login_options(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let mut timing = CeremonyTimer::new("login_options");
    auth::origin(h, &s.config)?;
    let (src, factor) = source(h);
    auth::rate(&s.pool, &format!("passkey-options:src:{src}"), 60 * factor).await?;
    timing.section("origin_rate");
    let (id, challenge) = new_challenge(&s.pool, "LOGIN", None, None).await?;
    timing.section("challenge");
    Ok(json!({
        "challenge_id": id,
        "public_key": {
            "challenge": challenge,
            "rpId": rp_id(s)?,
            "timeout": CHALLENGE_SECONDS * 1000,
            "userVerification": "required",
            "allowCredentials": [],
        }
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginInput {
    pub challenge_id: Uuid,
    /// The email entered before choosing the passkey; must be the passkey's account.
    pub email: Option<String>,
    pub credential_id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    pub signature: String,
    pub user_handle: Option<String>,
}

struct Verified {
    passkey: Uuid,
    user: Uuid,
    sign_count: u32,
    backed_up: bool,
    backup_eligible: bool,
}

pub async fn login(s: &AppState, h: &HeaderMap, i: LoginInput) -> Result<(HeaderMap, Json<Value>)> {
    authenticate(s, h, i, None).await
}

pub async fn step_up(
    s: &AppState,
    h: &HeaderMap,
    i: LoginInput,
) -> Result<(HeaderMap, Json<Value>)> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    authenticate(s, h, i, Some(a)).await
}

pub async fn step_up_options(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("passkey-step-up:{}", a.user), 20).await?;
    let (id, challenge) =
        new_challenge(&s.pool, "STEP_UP", Some(a.user), Some(&a.session_hash)).await?;
    Ok(
        json!({"challenge_id":id,"public_key":{"challenge":challenge,"rpId":rp_id(s)?,
        "timeout":CHALLENGE_SECONDS*1000,"userVerification":"required","allowCredentials":[]}}),
    )
}

/// Consume in the protected operation's transaction. Proofs are session-bound,
/// expire after five minutes and cannot authorize a second operation.
pub(crate) async fn consume_proof(
    c: &mut sqlx::PgConnection,
    a: &auth::Actor,
    proof: &str,
) -> Result<()> {
    if proof.len() != 64 {
        return Err(Error::InvalidCode("PASSKEY_STEP_UP_REQUIRED"));
    }
    // Match session-management lock order: account, then session, then proof.
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
        .bind(a.user)
        .execute(&mut *c)
        .await?;
    auth::lock_current_session(c, a).await?;
    let n = sqlx::query("DELETE FROM identity.passkey_proofs WHERE token_hash=$1 AND session_hash=$2 AND user_id=$3 AND expires_at>clock_timestamp()")
        .bind(auth::hash_token(proof)).bind(&a.session_hash).bind(a.user).execute(c).await?.rows_affected();
    if n != 1 {
        return Err(Error::InvalidCode("PASSKEY_STEP_UP_REQUIRED"));
    }
    Ok(())
}

async fn authenticate(
    s: &AppState,
    h: &HeaderMap,
    i: LoginInput,
    actor: Option<auth::Actor>,
) -> Result<(HeaderMap, Json<Value>)> {
    let mut timing = CeremonyTimer::new(if actor.is_some() { "step_up" } else { "login" });
    auth::origin(h, &s.config)?;
    let (src, factor) = source(h);
    auth::rate(
        &s.pool,
        &format!("passkey-login:src:{src}"),
        auth::LOGIN_PER_SOURCE * factor,
    )
    .await?;
    let (Some(cred), Some(cdj), Some(ad), Some(sig)) = (
        decode(&i.credential_id, 1023),
        decode(&i.client_data_json, 4096),
        decode(&i.authenticator_data, 1024),
        decode(&i.signature, 1024),
    ) else {
        return Err(Error::Unauthorized);
    };
    timing.section("origin_rate_decode");
    // Reuse the lookup connection for the transaction; all locks and checks
    // still run in their original order and READ COMMITTED statements.
    let mut connection = s.pool.acquire().await?;
    let email_user = match i.email.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => Some(crate::pii::user_by_email(&s.payout_keys, &mut *connection, e).await?),
        None => None,
    };
    timing.section("email");
    let rp = rp_id(s)?;
    let mut tx = sqlx::Connection::begin(&mut *connection).await?;
    let Some(challenge_hash) = consume(
        &mut tx,
        i.challenge_id,
        if actor.is_some() { "STEP_UP" } else { "LOGIN" },
        actor.as_ref().map(|a| a.user),
        actor.as_ref().map(|a| a.session_hash.as_slice()),
    )
    .await?
    else {
        return Err(Error::InvalidCode("PASSKEY_CHALLENGE_INVALID"));
    };
    timing.section("begin_consume");
    // Acquire the account lock before the credential lock, matching removal
    // and session revocation. Otherwise session issuance's user FK can deadlock
    // with a simultaneous credential removal.
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT u.id FROM identity.users u JOIN identity.passkeys p ON p.user_id=u.id WHERE p.credential_id=$1 AND u.status='ACTIVE' FOR SHARE OF u")
        .bind(&cred).fetch_optional(&mut *tx).await?;
    if let Some(a) = &actor
        && owner == Some(a.user)
    {
        auth::lock_current_session(&mut tx, a).await?;
    }
    let row = sqlx::query(
        "SELECT p.id,p.user_id,p.public_key,p.alg,p.sign_count FROM identity.passkeys p
           JOIN identity.users u ON u.id=p.user_id AND u.status='ACTIVE'
          WHERE p.credential_id=$1 FOR UPDATE OF p",
    )
    .bind(&cred)
    .fetch_optional(&mut *tx)
    .await?;
    timing.section("owner_credential_locks");
    let verified = row.and_then(|r| {
        let user: Uuid = r.get("user_id");
        let stored: i64 = r.get("sign_count");
        let data = auth_data(&ad, &rp)?;
        let ok = owner == Some(user)
            && actor.as_ref().is_none_or(|a| a.user == user)
            && email_user.is_none_or(|e| e == Some(user))
            && i.user_handle
                .as_deref()
                .is_none_or(|u| decode(u, 64).as_deref() == Some(user.as_bytes().as_slice()))
            && client_data_ok(&cdj, "webauthn.get", &challenge_hash, &origins(s))
            && data.flags & FLAG_UP != 0
            && data.flags & FLAG_UV != 0
            && signature_ok(r.get("alg"), r.get::<&[u8], _>("public_key"), &ad, &cdj, &sig)
            // A counter that does not move forward means a cloned authenticator
            // (synced passkeys report 0 throughout).
            && (data.sign_count == 0 && stored == 0 || i64::from(data.sign_count) > stored);
        ok.then(|| Verified {
            passkey: r.get("id"),
            user,
            sign_count: data.sign_count,
            backed_up: data.flags & FLAG_BS != 0,
            backup_eligible: data.flags & FLAG_BE != 0,
        })
    });
    timing.section("verification");
    let Some(v) = verified else {
        operations::audit(
            &mut tx,
            None,
            None,
            None,
            "auth.login_failed",
            "PASSKEY",
            auth::request_id(h),
        )
        .await?;
        tx.commit().await?;
        return Err(Error::Unauthorized);
    };
    sqlx::query(
        "UPDATE identity.passkeys SET sign_count=$2,backed_up=$3,backup_eligible=$4,last_used_at=now() WHERE id=$1",
    )
    .bind(v.passkey)
    .bind(i64::from(v.sign_count))
    .bind(v.backed_up)
    .bind(v.backup_eligible)
    .execute(&mut *tx)
    .await?;
    timing.section("counter_update");
    if let Some(a) = actor {
        let token = auth::random_token();
        sqlx::query("DELETE FROM identity.passkey_proofs WHERE expires_at<now()")
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO identity.passkey_proofs(token_hash,session_hash,user_id,expires_at) VALUES($1,$2,$3,now()+interval '5 minutes')")
            .bind(auth::hash_token(&token)).bind(&a.session_hash).bind(a.user).execute(&mut *tx).await?;
        operations::audit(
            &mut tx,
            Some(a.user),
            None,
            Some(a.user),
            "auth.passkey.step_up",
            "WEBAUTHN",
            a.request,
        )
        .await?;
        tx.commit().await?;
        return Ok((
            HeaderMap::new(),
            Json(json!({"reauth_token":token,"expires_in":300})),
        ));
    }
    let result = auth::issue_session(s, tx, h, v.user, "PASSKEY", true).await;
    timing.section("session_audit_commit");
    result
}

// ---------------------------------------------------------------------------
// Managing passkeys (signed in)
// ---------------------------------------------------------------------------

pub async fn list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, false).await?;
    list_for(&s.pool, a.user).await
}

async fn list_for<'e>(db: impl sqlx::PgExecutor<'e>, user: Uuid) -> Result<Value> {
    let rows = sqlx::query(
        "SELECT id,name,authenticator_name,backed_up,authenticator_attachment,transports,backup_eligible,created_at,last_used_at FROM identity.passkeys
          WHERE user_id=$1 ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(db)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "name": r.get::<String, _>("name"),
                "authenticator_name": r.get::<Option<String>, _>("authenticator_name"),
                "authenticator_attachment": r.get::<Option<String>, _>("authenticator_attachment"),
                "transports": r.get::<Option<Vec<String>>, _>("transports"),
                "backup_eligible": r.get::<Option<bool>, _>("backup_eligible"),
                "synced": r.get::<bool, _>("backed_up"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                "last_used_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at"),
            })
        })
        .collect();
    Ok(json!({"items": items, "limit": MAX_PER_USER}))
}

pub async fn register_options(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let mut timing = CeremonyTimer::new("register_options");
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("passkey-register:{}", a.user), 20).await?;
    timing.section("actor_rate");
    // Reuse a validated connection across the measured sequential metadata
    // and challenge reads, preserving their original statement order.
    let mut connection = s.pool.acquire().await?;
    let existing: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT credential_id FROM identity.passkeys WHERE user_id=$1")
            .bind(a.user)
            .fetch_all(&mut *connection)
            .await?;
    if existing.len() as i64 >= MAX_PER_USER {
        return Err(Error::PolicyGate("PASSKEY_LIMIT"));
    }
    timing.section("credentials");
    let email = crate::pii::user_email(&s.payout_keys, &mut *connection, a.user).await?;
    timing.section("email");
    let (id, challenge) = new_challenge(&mut *connection, "REGISTER", Some(a.user), None).await?;
    timing.section("challenge");
    let params: Vec<Value> = ALGS
        .iter()
        .map(|alg| json!({"type": "public-key", "alg": alg}))
        .collect();
    let exclude: Vec<Value> = existing
        .iter()
        .map(|c| json!({"type": "public-key", "id": B64.encode(c)}))
        .collect();
    Ok(json!({
        "challenge_id": id,
        "public_key": {
            "challenge": challenge,
            "rp": {"id": rp_id(s)?, "name": "AUDENIQ"},
            "user": {"id": B64.encode(a.user.as_bytes()), "name": email, "displayName": email},
            "pubKeyCredParams": params,
            "timeout": CHALLENGE_SECONDS * 1000,
            "attestation": "none",
            "authenticatorSelection": {
                "residentKey": "required",
                "requireResidentKey": true,
                "userVerification": "required",
            },
            "excludeCredentials": exclude,
        }
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterInput {
    pub challenge_id: Uuid,
    #[serde(default)]
    pub current_password: String,
    pub reauth_token: Option<String>,
    pub name: String,
    pub credential_id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    /// `AuthenticatorAttestationResponse.getPublicKey()` (SPKI DER).
    pub public_key: String,
    /// `getPublicKeyAlgorithm()` (COSE).
    pub alg: i32,
    /// Browser-reported display hints only; never used for authentication decisions.
    pub authenticator_attachment: Option<String>,
    pub transports: Option<Vec<String>>,
}

pub async fn register(s: &AppState, h: &HeaderMap, i: RegisterInput) -> Result<Value> {
    let mut timing = CeremonyTimer::new(if i.reauth_token.is_some() {
        "register_proof"
    } else {
        "register_password"
    });
    let current_password = zeroize::Zeroizing::new(i.current_password);
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("passkey-register:{}", a.user), 20).await?;
    let name = i.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 60 || current_password.len() > 128 {
        return Err(Error::Invalid);
    }
    if i.authenticator_attachment
        .as_deref()
        .is_some_and(|v| !matches!(v, "platform" | "cross-platform"))
        || i.transports.as_ref().is_some_and(|values| {
            values.len() > 5
                || values
                    .iter()
                    .any(|v| !matches!(v.as_str(), "usb" | "nfc" | "ble" | "hybrid" | "internal"))
        })
    {
        return Err(Error::InvalidCode("PASSKEY_INVALID"));
    }
    crate::text_policy::check(&name)?;
    let (Some(cred), Some(cdj), Some(ad), Some(spki)) = (
        decode(&i.credential_id, 1023),
        decode(&i.client_data_json, 4096),
        decode(&i.authenticator_data, 4096),
        decode(&i.public_key, 2048),
    ) else {
        return Err(Error::InvalidCode("PASSKEY_INVALID"));
    };
    timing.section("actor_rate_decode");
    // A fresh passkey proof can replace emergency password verification.
    let password_hash = if i.reauth_token.is_none() {
        let hash: String = sqlx::query_scalar(
            "SELECT password_hash FROM identity.users WHERE id=$1 AND status='ACTIVE'",
        )
        .bind(a.user)
        .fetch_optional(&s.pool)
        .await?
        .ok_or(Error::Unauthorized)?;
        let permit = s
            .password_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Internal)?;
        let password = current_password;
        let verified_hash = hash.clone();
        let valid =
            crate::password_work::run(permit, move || auth::verify_password(&password, &hash))
                .await
                .map(|(_, ok)| ok)
                .unwrap_or(false);
        if !valid {
            return Err(Error::InvalidCode("CURRENT_PASSWORD_INVALID"));
        }
        Some(verified_hash)
    } else {
        None
    };
    timing.section("reauth_password");
    let rp = rp_id(s)?;
    // Acquire only after password work, and reuse after commit for the response
    // list. No pooled connection is held during Argon2 or its semaphore wait.
    let mut connection = s.pool.acquire().await?;
    let mut tx = sqlx::Connection::begin(&mut *connection).await?;
    // One registration at a time per account (the limit below holds).
    if let Some(proof) = &i.reauth_token {
        consume_proof(&mut tx, &a, proof).await?;
    } else {
        sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
            .bind(a.user)
            .execute(&mut *tx)
            .await?;
        auth::lock_current_session(&mut tx, &a).await?;
    }
    if let Some(hash) = password_hash {
        auth::current_password(&mut tx, a.user, &hash).await?;
    }
    timing.section("transaction_reauth_locks");
    let Some(challenge_hash) =
        consume(&mut tx, i.challenge_id, "REGISTER", Some(a.user), None).await?
    else {
        return Err(Error::InvalidCode("PASSKEY_CHALLENGE_INVALID"));
    };
    timing.section("consume");
    let data = auth_data(&ad, &rp);
    // Passkeys are added in the studio only (the admin site cannot reach this route).
    let ok = client_data_ok(
        &cdj,
        "webauthn.create",
        &challenge_hash,
        &[s.config.origin.as_str()],
    ) && data.as_ref().is_some_and(|d| {
        d.flags & FLAG_UP != 0
            && d.flags & FLAG_UV != 0
            && d.credential_id.is_some_and(|c| bool::from(c.ct_eq(&cred)))
    }) && parsed_key(i.alg, &spki).is_some();
    if !ok {
        tx.commit().await?;
        return Err(Error::InvalidCode("PASSKEY_INVALID"));
    }
    let data = data.ok_or(Error::Internal)?;
    timing.section("verification");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.passkeys WHERE user_id=$1")
        .bind(a.user)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_PER_USER {
        return Err(Error::PolicyGate("PASSKEY_LIMIT"));
    }
    timing.section("count");
    let authenticator_name = data.aaguid.and_then(authenticator_name);
    let name = authenticator_name.unwrap_or(&name);
    let inserted = sqlx::query(
        "INSERT INTO identity.passkeys(id,user_id,credential_id,public_key,alg,sign_count,backed_up,name,aaguid,authenticator_name,authenticator_attachment,transports,backup_eligible)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) ON CONFLICT(credential_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(a.user)
    .bind(&cred)
    .bind(&spki)
    .bind(i.alg)
    .bind(i64::from(data.sign_count))
    .bind(data.flags & FLAG_BS != 0)
    .bind(name)
    .bind(data.aaguid.filter(|id| !id.is_nil()))
    .bind(authenticator_name)
    .bind(i.authenticator_attachment)
    .bind(i.transports)
    .bind(data.flags & FLAG_BE != 0)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted != 1 {
        return Err(Error::InvalidCode("PASSKEY_ALREADY_REGISTERED"));
    }
    timing.section("insert");
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "auth.passkey.added",
        "WEBAUTHN",
        a.request,
    )
    .await?;
    timing.section("audit");
    tx.commit().await?;
    timing.section("commit");
    let result = list_for(&mut *connection, a.user).await;
    timing.section("list");
    result
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameInput {
    pub name: String,
}

pub async fn rename(s: &AppState, h: &HeaderMap, id: Uuid, i: RenameInput) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    let name = i.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Error::Invalid);
    }
    crate::text_policy::check(name)?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    auth::lock_current_session(&mut tx, &a).await?;
    let n = sqlx::query("UPDATE identity.passkeys SET name=$3 WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(a.user)
        .bind(name)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "auth.passkey.renamed",
        "WEBAUTHN",
        a.request,
    )
    .await?;
    tx.commit().await?;
    list_for(&s.pool, a.user).await
}

pub async fn remove(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    // Shared with OAuth unlink and registration: concurrent removals cannot
    // each observe the other credential and strand the account.
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE")
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    // Evaluate alternatives in a new READ COMMITTED statement after the lock
    // has been acquired; subqueries in the locking statement can see stale rows.
    let alternatives: bool = sqlx::query_scalar("SELECT password_hash LIKE '$argon2%' OR EXISTS(SELECT 1 FROM identity.oauth_identities WHERE user_id=$1) OR EXISTS(SELECT 1 FROM identity.passkeys WHERE user_id=$1 AND id<>$2) FROM identity.users WHERE id=$1")
        .bind(a.user).bind(id).fetch_one(&mut *tx).await?;
    auth::lock_current_session(&mut tx, &a).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM identity.passkeys WHERE id=$1 AND user_id=$2)",
    )
    .bind(id)
    .bind(a.user)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(Error::NotFound);
    }
    if !alternatives {
        return Err(Error::PolicyGate("PASSKEY_LAST_SIGN_IN_METHOD"));
    }
    let n = sqlx::query("DELETE FROM identity.passkeys WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(a.user)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "auth.passkey.removed",
        "WEBAUTHN",
        a.request,
    )
    .await?;
    tx.commit().await?;
    list_for(&s.pool, a.user).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aaguid_registry_and_truncated_attested_data() {
        for (id, name) in AUTHENTICATORS {
            assert_eq!(authenticator_name(*id), Some(*name));
            let mut raw = Sha256::digest(b"localhost").to_vec();
            raw.push(FLAG_AT | FLAG_UP | FLAG_UV);
            raw.extend_from_slice(&0u32.to_be_bytes());
            raw.extend_from_slice(id.as_bytes());
            raw.extend_from_slice(&1u16.to_be_bytes());
            raw.push(42);
            assert_eq!(auth_data(&raw, "localhost").unwrap().aaguid, Some(*id));
            for end in 37..raw.len() {
                assert!(auth_data(&raw[..end], "localhost").is_none());
            }
        }
        assert_eq!(authenticator_name(Uuid::nil()), None);
        assert_eq!(authenticator_name(Uuid::from_u128(1)), None);
        let mut assertion = Sha256::digest(b"localhost").to_vec();
        assertion.push(FLAG_UP | FLAG_UV);
        assertion.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(auth_data(&assertion, "localhost").unwrap().aaguid, None);
    }

    #[sqlx::test]
    async fn challenge_expiry_uses_wall_clock_and_locked_challenges_do_not_wait(
        pool: sqlx::PgPool,
    ) {
        crate::database::MIGRATOR.run(&pool).await.unwrap();
        let id = Uuid::new_v4();
        let mut old_tx = pool.begin().await.unwrap();
        sqlx::query("SELECT now()")
            .execute(&mut *old_tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO identity.webauthn_challenges(id,challenge_hash,purpose,expires_at) VALUES($1,$2,'LOGIN',clock_timestamp()+interval '150 milliseconds')")
            .bind(id).bind(vec![1u8;32]).execute(&pool).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let (transaction_would_accept, wall_clock_would_accept): (bool, bool) = sqlx::query_as(
            "SELECT expires_at>now(),expires_at>clock_timestamp() FROM identity.webauthn_challenges WHERE id=$1")
            .bind(id).fetch_one(&mut *old_tx).await.unwrap();
        assert!(
            transaction_would_accept,
            "reproduces the stale transaction clock"
        );
        assert!(!wall_clock_would_accept);
        assert!(
            consume(&mut old_tx, id, "LOGIN", None, None)
                .await
                .unwrap()
                .is_none()
        );
        old_tx.rollback().await.unwrap();
        sqlx::query("UPDATE identity.webauthn_challenges SET expires_at=clock_timestamp()+interval '300 seconds' WHERE id=$1")
            .bind(id).execute(&pool).await.unwrap();
        let mut holder = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM identity.webauthn_challenges WHERE id=$1 FOR UPDATE")
            .bind(id)
            .execute(&mut *holder)
            .await
            .unwrap();
        let mut contender = pool.begin().await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            consume(&mut contender, id, "LOGIN", None, None),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.is_none());
        contender.rollback().await.unwrap();
        holder.rollback().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            consume(&mut tx, id, "LOGIN", None, None)
                .await
                .unwrap()
                .is_some()
        );
        tx.commit().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            consume(&mut tx, id, "LOGIN", None, None)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rp_id_is_the_site_host_or_a_parent_of_every_site() {
        assert_eq!(
            pick_rp_id("studio.audeniq.com", None, None).as_deref(),
            Some("studio.audeniq.com")
        );
        assert_eq!(
            pick_rp_id(
                "studio.audeniq.com",
                Some("admin.audeniq.com"),
                Some(" Audeniq.com ")
            )
            .as_deref(),
            Some("audeniq.com")
        );
        // Not a parent of the admin host, or a lookalike suffix: refused.
        assert_eq!(
            pick_rp_id(
                "studio.audeniq.com",
                Some("admin.other.com"),
                Some("audeniq.com")
            ),
            None
        );
        assert_eq!(
            pick_rp_id("studio.notaudeniq.com", None, Some("audeniq.com")),
            None
        );
    }
}
