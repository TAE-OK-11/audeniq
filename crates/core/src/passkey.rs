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
//! - Adding a passkey needs the current password, so a stolen session cannot
//!   plant its own login.
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

const CHALLENGE_SECONDS: i32 = 300;
const MAX_PER_USER: i64 = 10;
const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_BS: u8 = 0x10;
const FLAG_AT: u8 = 0x40;
/// COSE algorithm ids, preferred first.
const ALGS: [i32; 3] = [-8, -7, -257];

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

async fn new_challenge(s: &AppState, purpose: &str, user: Option<Uuid>) -> Result<(Uuid, String)> {
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
        "DELETE FROM identity.webauthn_challenges WHERE expires_at < now() - interval '1 hour'",
    )
    .execute(&s.pool)
    .await?;
    sqlx::query(
        "INSERT INTO identity.webauthn_challenges(id,challenge_hash,purpose,user_id,expires_at)
         VALUES($1,$2,$3,$4,now()+make_interval(secs=>$5))",
    )
    .bind(id)
    .bind(Sha256::digest(challenge.as_bytes()).to_vec())
    .bind(purpose)
    .bind(user)
    .bind(f64::from(CHALLENGE_SECONDS))
    .execute(&s.pool)
    .await?;
    Ok((id, challenge))
}

/// Marks the challenge used (once only) and returns its hash.
async fn consume(
    c: &mut sqlx::PgConnection,
    id: Uuid,
    purpose: &str,
    user: Option<Uuid>,
) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        "UPDATE identity.webauthn_challenges SET consumed_at=now()
          WHERE id=$1 AND purpose=$2 AND user_id IS NOT DISTINCT FROM $3
            AND consumed_at IS NULL AND expires_at>now() RETURNING challenge_hash",
    )
    .bind(id)
    .bind(purpose)
    .bind(user)
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
    auth::origin(h, &s.config)?;
    let (src, factor) = source(h);
    auth::rate(&s.pool, &format!("passkey-options:src:{src}"), 60 * factor).await?;
    let (id, challenge) = new_challenge(s, "LOGIN", None).await?;
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
}

pub async fn login(s: &AppState, h: &HeaderMap, i: LoginInput) -> Result<(HeaderMap, Json<Value>)> {
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
    let email_user = match i.email.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => Some(crate::pii::user_by_email(&s.payout_keys, &s.pool, e).await?),
        None => None,
    };
    let rp = rp_id(s)?;
    let mut tx = s.pool.begin().await?;
    let Some(challenge_hash) = consume(&mut tx, i.challenge_id, "LOGIN", None).await? else {
        return Err(Error::InvalidCode("PASSKEY_CHALLENGE_INVALID"));
    };
    let row = sqlx::query(
        "SELECT p.id,p.user_id,p.public_key,p.alg,p.sign_count FROM identity.passkeys p
           JOIN identity.users u ON u.id=p.user_id AND u.status='ACTIVE'
          WHERE p.credential_id=$1 FOR UPDATE OF p",
    )
    .bind(&cred)
    .fetch_optional(&mut *tx)
    .await?;
    let verified = row.and_then(|r| {
        let user: Uuid = r.get("user_id");
        let stored: i64 = r.get("sign_count");
        let data = auth_data(&ad, &rp)?;
        let ok = email_user.is_none_or(|e| e == Some(user))
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
        })
    });
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
        "UPDATE identity.passkeys SET sign_count=$2,backed_up=$3,last_used_at=now() WHERE id=$1",
    )
    .bind(v.passkey)
    .bind(i64::from(v.sign_count))
    .bind(v.backed_up)
    .execute(&mut *tx)
    .await?;
    auth::issue_session(s, tx, h, v.user, "PASSKEY", true).await
}

// ---------------------------------------------------------------------------
// Managing passkeys (signed in)
// ---------------------------------------------------------------------------

pub async fn list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, false).await?;
    list_for(s, a.user).await
}

async fn list_for(s: &AppState, user: Uuid) -> Result<Value> {
    let rows = sqlx::query(
        "SELECT id,name,backed_up,created_at,last_used_at FROM identity.passkeys
          WHERE user_id=$1 ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(&s.pool)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "name": r.get::<String, _>("name"),
                "synced": r.get::<bool, _>("backed_up"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                "last_used_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at"),
            })
        })
        .collect();
    Ok(json!({"items": items, "limit": MAX_PER_USER}))
}

pub async fn register_options(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("passkey-register:{}", a.user), 20).await?;
    let existing: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT credential_id FROM identity.passkeys WHERE user_id=$1")
            .bind(a.user)
            .fetch_all(&s.pool)
            .await?;
    if existing.len() as i64 >= MAX_PER_USER {
        return Err(Error::PolicyGate("PASSKEY_LIMIT"));
    }
    let email = crate::pii::user_email(&s.payout_keys, &s.pool, a.user).await?;
    let (id, challenge) = new_challenge(s, "REGISTER", Some(a.user)).await?;
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
    pub current_password: String,
    pub name: String,
    pub credential_id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    /// `AuthenticatorAttestationResponse.getPublicKey()` (SPKI DER).
    pub public_key: String,
    /// `getPublicKeyAlgorithm()` (COSE).
    pub alg: i32,
}

pub async fn register(s: &AppState, h: &HeaderMap, i: RegisterInput) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("passkey-register:{}", a.user), 20).await?;
    let name = i.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 60 || i.current_password.len() > 128 {
        return Err(Error::Invalid);
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
    // Re-authenticate with the password before binding a new authenticator.
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
    let password = i.current_password;
    let valid = crate::password_work::run(permit, move || auth::verify_password(&password, &hash))
        .await
        .map(|(_, ok)| ok)
        .unwrap_or(false);
    if !valid {
        return Err(Error::InvalidCode("CURRENT_PASSWORD_INVALID"));
    }
    let rp = rp_id(s)?;
    let mut tx = s.pool.begin().await?;
    // One registration at a time per account (the limit below holds).
    sqlx::query("SELECT 1 FROM identity.users WHERE id=$1 FOR UPDATE")
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    let Some(challenge_hash) = consume(&mut tx, i.challenge_id, "REGISTER", Some(a.user)).await?
    else {
        return Err(Error::InvalidCode("PASSKEY_CHALLENGE_INVALID"));
    };
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
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.passkeys WHERE user_id=$1")
        .bind(a.user)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_PER_USER {
        return Err(Error::PolicyGate("PASSKEY_LIMIT"));
    }
    let inserted = sqlx::query(
        "INSERT INTO identity.passkeys(id,user_id,credential_id,public_key,alg,sign_count,backed_up,name)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(credential_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(a.user)
    .bind(&cred)
    .bind(&spki)
    .bind(i.alg)
    .bind(i64::from(data.sign_count))
    .bind(data.flags & FLAG_BS != 0)
    .bind(&name)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted != 1 {
        return Err(Error::InvalidCode("PASSKEY_ALREADY_REGISTERED"));
    }
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
    tx.commit().await?;
    list_for(s, a.user).await
}

pub async fn remove(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
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
    list_for(s, a.user).await
}

#[cfg(test)]
mod tests {
    use super::pick_rp_id;

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
