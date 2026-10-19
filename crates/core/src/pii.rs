//! Personal data sealed with the KMS-protected keyring ([`crate::payout_keys`]).
//!
//! Rows written before migration 0102 keep their plaintext until
//! `audeniq-admin privacy encrypt-pii` seals them, so every read accepts either
//! form. New values are only ever written sealed.
//!
//! Queries that build JSON in SQL embed a marker instead of the value
//! (see [`email_ref!`]); [`reveal`] replaces each marker after decryption.
//! Ciphertext travels hex-encoded (PostgreSQL wraps base64 at 76 columns).
//! A marker that is not revealed carries only ciphertext.
use crate::{
    error::{Error, Result},
    payout_keys::{Field, KeyRing},
};
use serde_json::Value;
use uuid::Uuid;

pub const MARKER: &str = "$pii";

/// SQL for a JSON marker: field name, subject id, plaintext and ciphertext
/// expressions (all string literals).
#[macro_export]
macro_rules! pii_ref {
    ($field:expr, $subject:expr, $plain:expr, $cipher:expr) => {
        concat!(
            "jsonb_build_object('$pii','",
            $field,
            "','s',",
            $subject,
            ",'p',",
            $plain,
            ",'c',encode(",
            $cipher,
            ",'hex'))"
        )
    };
}

/// SQL for a JSON marker of a user's login email; `$u` is the users alias.
#[macro_export]
macro_rules! email_ref {
    ($u:literal) => {
        $crate::pii_ref!(
            "user_email",
            concat!($u, ".id"),
            concat!($u, ".email"),
            concat!($u, ".email_cipher")
        )
    };
}

fn field(name: &str) -> Option<Field> {
    Some(match name {
        "user_email" => Field::UserEmail,
        "profile_contact_email" => Field::ProfileContactEmail,
        "contact_name" => Field::ContactName,
        "contact_email" => Field::ContactEmail,
        "contact_phone" => Field::ContactPhone,
        "holder_name" => Field::PayoutHolderName,
        _ => return None,
    })
}

/// Plaintext (pre-0102 row) or sealed value; neither yields `None`.
pub fn open(
    keys: &KeyRing,
    field: Field,
    subject: Uuid,
    plain: Option<String>,
    cipher: Option<&[u8]>,
) -> Result<Option<String>> {
    match cipher {
        Some(c) => Ok(Some(keys.open_field(field, subject, c)?.to_string())),
        None => Ok(plain),
    }
}

/// Seals an optional value; empty strings stay absent.
pub fn seal_opt(
    keys: &KeyRing,
    field: Field,
    subject: Uuid,
    value: Option<&str>,
) -> Result<Option<Vec<u8>>> {
    value
        .filter(|v| !v.is_empty())
        .map(|v| keys.seal_field(field, subject, v))
        .transpose()
}

/// Replaces every `{"$pii": field, "s": subject, "p": plain, "c": hex}`
/// object in `value` with the revealed string (or null).
pub fn reveal(keys: &KeyRing, value: &mut Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(name)) = map.get(MARKER) {
                let field = field(name).ok_or(Error::Internal)?;
                let plain = map.get("p").and_then(Value::as_str).map(str::to_owned);
                // A missing row (LEFT JOIN) has no subject and no value.
                let revealed = match map.get("c").and_then(Value::as_str) {
                    None => plain,
                    Some(c) => {
                        let cipher = crate::hex::decode(c).map_err(|_| Error::Internal)?;
                        let subject: Uuid = map
                            .get("s")
                            .and_then(Value::as_str)
                            .and_then(|s| s.parse().ok())
                            .ok_or(Error::Internal)?;
                        open(keys, field, subject, None, Some(&cipher))?
                    }
                };
                *value = revealed.map_or(Value::Null, Value::String);
                return Ok(());
            }
            for v in map.values_mut() {
                reveal(keys, v)?;
            }
        }
        Value::Array(items) => {
            for v in items {
                reveal(keys, v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Sealed login email and its blind index under the active key version.
pub fn seal_email(keys: &KeyRing, user: Uuid, email: &str) -> Result<(Vec<u8>, String)> {
    Ok((
        keys.seal_field(Field::UserEmail, user, email)?,
        keys.blind_index(Field::UserEmail, email)?,
    ))
}

/// Blind indexes to match a (normalized) login email under every key version.
/// Use with `(u.email=$plain OR u.email_bidx=ANY($indexes))`.
pub fn email_indexes(keys: &KeyRing, email: &str) -> Vec<String> {
    keys.blind_indexes(Field::UserEmail, email)
}

/// Keyring for owner/operator CLIs (KMS mode requires the tmpfs keyring file).
pub fn cli_keys() -> Result<KeyRing> {
    KeyRing::from_env(crate::config::kms_enabled()?)
}

/// The user with this login email (normalized to lower case), in either form.
pub async fn user_by_email<'e>(
    keys: &KeyRing,
    db: impl sqlx::PgExecutor<'e>,
    email: &str,
) -> Result<Option<Uuid>> {
    let email = email.trim().to_lowercase();
    Ok(
        sqlx::query_scalar("SELECT id FROM identity.users WHERE email=$1 OR email_bidx=ANY($2)")
            .bind(&email)
            .bind(email_indexes(keys, &email))
            .fetch_optional(db)
            .await?,
    )
}

/// A user's login email, decrypted.
pub async fn user_email<'e>(
    keys: &KeyRing,
    db: impl sqlx::PgExecutor<'e>,
    user: Uuid,
) -> Result<String> {
    let (plain, cipher): (Option<String>, Option<Vec<u8>>) =
        sqlx::query_as("SELECT email,email_cipher FROM identity.users WHERE id=$1")
            .bind(user)
            .fetch_one(db)
            .await?;
    open(keys, Field::UserEmail, user, plain, cipher.as_deref())?.ok_or(Error::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keys() -> KeyRing {
        KeyRing::test_ring()
    }

    #[test]
    fn markers_reveal_sealed_and_plain_values() {
        let keys = keys();
        let user = Uuid::new_v4();
        let sealed = keys
            .seal_field(Field::UserEmail, user, "a@example.com")
            .unwrap();
        let mut v = json!({
            "rows": [
                {"email": {"$pii": "user_email", "s": user, "p": null, "c": crate::hex::encode(&sealed)}},
                {"email": {"$pii": "user_email", "s": Uuid::new_v4(), "p": "old@example.com", "c": null}},
                {"email": {"$pii": "user_email", "s": Uuid::new_v4(), "p": null, "c": null}},
            ]
        });
        reveal(&keys, &mut v).unwrap();
        assert_eq!(v["rows"][0]["email"], "a@example.com");
        assert_eq!(v["rows"][1]["email"], "old@example.com");
        assert_eq!(v["rows"][2]["email"], Value::Null);
    }

    #[test]
    fn markers_fail_closed_for_another_subject_or_field() {
        let keys = keys();
        let user = Uuid::new_v4();
        let sealed = crate::hex::encode(
            keys.seal_field(Field::UserEmail, user, "a@example.com")
                .unwrap(),
        );
        let mut moved = json!({"$pii": "user_email", "s": Uuid::new_v4(), "c": sealed});
        assert!(reveal(&keys, &mut moved).is_err());
        let mut other = json!({"$pii": "contact_email", "s": user, "c": sealed});
        assert!(reveal(&keys, &mut other).is_err());
        let mut unknown = json!({"$pii": "nope", "s": user, "c": sealed});
        assert!(reveal(&keys, &mut unknown).is_err());
    }

    #[test]
    fn blind_index_is_keyed_per_field_and_matches_after_rotation() {
        let old = KeyRing::test_ring_active(1);
        let new = KeyRing::test_ring_active(2);
        let stored = old.blind_index(Field::UserEmail, "a@example.com").unwrap();
        assert_ne!(
            stored,
            new.blind_index(Field::UserEmail, "a@example.com").unwrap()
        );
        assert!(email_indexes(&new, "a@example.com").contains(&stored));
        assert!(!email_indexes(&new, "b@example.com").contains(&stored));
        assert_ne!(
            stored,
            old.blind_index(Field::ContactEmail, "a@example.com")
                .unwrap()
        );
        assert!(!stored.contains("example"));
    }
}
