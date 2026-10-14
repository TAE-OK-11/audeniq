//! Versioned AES-256-GCM keys, separated from the database and API environment.
//! Optional KMS mode requires a read-only keyring materialized in tmpfs.
//!
//! The same keyring protects bank account numbers and the other personal data
//! listed in [`Field`]. Each field has its own AAD prefix, so a ciphertext
//! cannot be moved to another column or another subject. Columns that must be
//! looked up by value also store a keyed blind index (HMAC-SHA256 under a key
//! derived from each keyring version).
use crate::error::{Error, Result};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{AeadInOut, Generate},
};
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub const TAG: &[u8] = b"AQAC\0\x02";
const HEADER_LEN: usize = TAG.len() + 2;
/// Derivation label of the document sealing (Ed25519) key per keyring version.
const SIGN_KEY_LABEL: &[u8] = b"audeniq:document-signing-key:v1";
const NONCE_LEN: usize = 12;
const AUTH_TAG_LEN: usize = 16;
const AAD_PREFIX: &[u8] = b"audeniq:portal.payout_accounts:account_number:";
const INDEX_KEY_LABEL: &[u8] = b"audeniq:blind-index-key:v1";

/// Personal data sealed with the keyring. The AAD names the column and binds
/// the row's subject id (user, org or contact id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    UserEmail,
    IdentityRealName,
    IdentityBirthDate,
    PayoutHolderName,
    ProfileContactEmail,
    ContactName,
    ContactEmail,
    ContactPhone,
    TaxTin,
    StaffTotpSecret,
    OAuthEmail,
    PrintSource,
    ArchiveObject,
    ClaimantName,
    ClaimantEmail,
    ClaimantPhone,
    ReleaseAlertEmail,
}
impl Field {
    fn aad_prefix(self) -> &'static [u8] {
        match self {
            Self::UserEmail => b"audeniq:identity.users:email:",
            Self::IdentityRealName => b"audeniq:portal.account_identities:real_name:",
            Self::IdentityBirthDate => b"audeniq:portal.account_identities:birth_date:",
            Self::PayoutHolderName => b"audeniq:portal.payout_accounts:holder_name:",
            Self::ProfileContactEmail => b"audeniq:portal.artist_profiles:contact_email:",
            Self::ContactName => b"audeniq:work.relationship_contacts:name:",
            Self::ContactEmail => b"audeniq:work.relationship_contacts:email:",
            Self::ContactPhone => b"audeniq:work.relationship_contacts:phone:",
            Self::TaxTin => b"audeniq:portal.tax_residencies:tin:",
            Self::StaffTotpSecret => b"audeniq:identity.staff_mfa:totp:",
            Self::OAuthEmail => b"audeniq:identity.oauth_identities:email:",
            Self::PrintSource => b"audeniq:operations.print_stamps:source:",
            Self::ArchiveObject => b"audeniq:operations.archive_objects:object:",
            Self::ClaimantName => b"audeniq:portal.infringement_claims:claimant_name:",
            Self::ClaimantEmail => b"audeniq:portal.infringement_claims:claimant_email:",
            Self::ClaimantPhone => b"audeniq:portal.infringement_claims:claimant_phone:",
            Self::ReleaseAlertEmail => b"audeniq:portal.release_alerts:email:",
        }
    }
}

pub struct KeyRing {
    active: i16,
    // Expand each key once. The AES/GHASH states zeroize on drop; plaintext
    // input keys are wiped immediately after construction.
    keys: BTreeMap<i16, Aes256Gcm>,
    // Blind-index keys, derived once per version (never the AES key itself).
    index_keys: BTreeMap<i16, Zeroizing<[u8; 32]>>,
    // Ed25519 seeds for sealing documents, derived per version (never the AES key).
    sign_seeds: BTreeMap<i16, Zeroizing<[u8; 32]>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFile {
    version: u8,
    active_version: i16,
    keys: BTreeMap<String, String>,
}
impl Drop for KeyFile {
    fn drop(&mut self) {
        for value in self.keys.values_mut() {
            value.zeroize();
        }
    }
}

impl KeyRing {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let file: KeyFile = serde_json::from_slice(bytes)
            .map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?;
        if file.version != 1
            || file.active_version <= 0
            || file.keys.is_empty()
            || file.keys.len() > 64
        {
            return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
        }
        let mut keys = BTreeMap::new();
        let mut index_keys = BTreeMap::new();
        let mut sign_seeds = BTreeMap::new();
        for (version, value) in &file.keys {
            let version: i16 = version
                .parse()
                .map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?;
            let value = Zeroizing::new(
                hex::decode(value).map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?,
            );
            if version <= 0 || value.len() != 32 {
                return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
            }
            if keys
                .insert(version, Self::prepare_cipher(&value)?)
                .is_some()
            {
                return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
            }
            index_keys.insert(version, Self::derive_index_key(&value));
            sign_seeds.insert(version, Self::derive_with(&value, SIGN_KEY_LABEL));
        }
        if !keys.contains_key(&file.active_version) {
            return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
        }
        Ok(Self {
            active: file.active_version,
            keys,
            index_keys,
            sign_seeds,
        })
    }

    pub fn from_env(require_file: bool) -> Result<Self> {
        let require_file = require_file || crate::config::kms_enabled()?;
        if let Some(path) = std::env::var("PAYOUT_KEYRING_FILE")
            .ok()
            .filter(|path| !path.is_empty())
        {
            #[cfg(unix)]
            let mut file = {
                use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(path)
                    .map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?;
                let m = file.metadata().map_err(|_| Error::Internal)?;
                // Root or this user owns it; other users cannot read it or edit it.
                let uid = unsafe { libc::geteuid() };
                let gid = unsafe { libc::getegid() };
                if !m.is_file()
                    || (m.uid() != 0 && m.uid() != uid)
                    || m.mode() & 0o037 != 0
                    || (uid != 0 && m.mode() & 0o040 != 0 && m.gid() != gid)
                {
                    return Err(Error::PolicyGate("PAYOUT_KEYRING_PERMISSIONS"));
                }
                #[cfg(target_os = "linux")]
                if require_file {
                    use std::os::fd::AsRawFd;
                    let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
                    if unsafe { libc::fstatfs(file.as_raw_fd(), fs.as_mut_ptr()) } != 0
                        || unsafe { fs.assume_init() }.f_type != 0x01021994
                    {
                        return Err(Error::PolicyGate("PAYOUT_KEYRING_REQUIRES_TMPFS"));
                    }
                }
                file
            };
            #[cfg(not(unix))]
            let mut file = std::fs::File::open(path)
                .map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?;
            let mut bytes = Zeroizing::new(Vec::new());
            (&mut file)
                .take(16385)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Internal)?;
            if bytes.len() > 16384 {
                return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
            }
            return Self::parse(&bytes);
        }
        if require_file {
            return Err(Error::PolicyGate("PAYOUT_KEYRING_REQUIRED"));
        }
        // KMS-off mode may use a local AES key. An absent key disables account
        // encryption operations, never storing account numbers as plaintext.
        Self::development_key(std::env::var("PAYOUT_ACCOUNT_KEY").ok())
    }

    fn development_key(key: Option<String>) -> Result<Self> {
        let (keys, index_keys, sign_seeds) = match key {
            Some(key) if !key.trim().is_empty() => {
                let key = Zeroizing::new(key);
                let decoded = Zeroizing::new(
                    hex::decode(key.trim())
                        .map_err(|_| Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"))?,
                );
                if decoded.len() != 32 {
                    return Err(Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"));
                }
                (
                    BTreeMap::from([(1, Self::prepare_cipher(&decoded)?)]),
                    BTreeMap::from([(1, Self::derive_index_key(&decoded))]),
                    BTreeMap::from([(1, Self::derive_with(&decoded, SIGN_KEY_LABEL))]),
                )
            }
            _ => (BTreeMap::new(), BTreeMap::new(), BTreeMap::new()),
        };
        Ok(Self {
            active: 1,
            keys,
            index_keys,
            sign_seeds,
        })
    }

    pub fn active_version(&self) -> i16 {
        self.active
    }
    #[cfg(test)]
    pub(crate) fn test_ring_active(active: i16) -> Self {
        Self::parse(
            format!(
                r#"{{"version":1,"active_version":{active},"keys":{{"1":"{}","2":"{}"}}}}"#,
                "11".repeat(32),
                "22".repeat(32)
            )
            .as_bytes(),
        )
        .unwrap()
    }
    #[cfg(test)]
    pub(crate) fn test_ring() -> Self {
        Self::test_ring_active(2)
    }
    /// False when no key is configured: personal data cannot be stored.
    pub fn ready(&self) -> bool {
        self.keys.contains_key(&self.active)
    }
    fn derive_with(key: &[u8], label: &[u8]) -> Zeroizing<[u8; 32]> {
        use hmac::{Hmac, KeyInit, Mac};
        let mut mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(key).expect("HMAC key");
        mac.update(label);
        Zeroizing::new(mac.finalize().into_bytes().into())
    }
    fn signer(&self, version: i16) -> Option<aws_lc_rs::signature::Ed25519KeyPair> {
        let seed = self.sign_seeds.get(&version)?;
        aws_lc_rs::signature::Ed25519KeyPair::from_seed_unchecked(&seed[..]).ok()
    }
    /// Ed25519 signature over `message` with the active version's document
    /// key: `(version, signature)`, or `None` when no key is configured.
    pub fn sign_document(&self, message: &[u8]) -> Option<(i16, Vec<u8>)> {
        let kp = self.signer(self.active)?;
        Some((self.active, kp.sign(message).as_ref().to_vec()))
    }
    /// Public document keys by version (published so anyone can verify).
    pub fn document_public_keys(&self) -> Vec<(i16, Vec<u8>)> {
        use aws_lc_rs::signature::KeyPair;
        self.sign_seeds
            .keys()
            .filter_map(|v| Some((*v, self.signer(*v)?.public_key().as_ref().to_vec())))
            .collect()
    }
    pub fn verify_document(&self, version: i16, message: &[u8], signature: &[u8]) -> bool {
        use aws_lc_rs::signature::{ED25519, KeyPair, UnparsedPublicKey};
        self.signer(version).is_some_and(|kp| {
            UnparsedPublicKey::new(&ED25519, kp.public_key().as_ref())
                .verify(message, signature)
                .is_ok()
        })
    }
    fn derive_index_key(key: &[u8]) -> Zeroizing<[u8; 32]> {
        use hmac::{Hmac, KeyInit, Mac};
        let mut mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(key).expect("HMAC key");
        mac.update(INDEX_KEY_LABEL);
        Zeroizing::new(mac.finalize().into_bytes().into())
    }
    fn index_with(key: &[u8; 32], field: Field, value: &str) -> String {
        use hmac::{Hmac, KeyInit, Mac};
        let mut mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(key).expect("HMAC key");
        mac.update(field.aad_prefix());
        mac.update(value.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
    /// Blind index under the active key version (stored with new rows).
    pub fn blind_index(&self, field: Field, value: &str) -> Result<String> {
        let key = self
            .index_keys
            .get(&self.active)
            .ok_or(Error::PolicyGate("PAYOUT_KEY_VERSION_MISSING"))?;
        Ok(Self::index_with(key, field, value))
    }
    /// Blind indexes under every available version, for lookups that must also
    /// find rows not yet re-encrypted after a rotation.
    pub fn blind_indexes(&self, field: Field, value: &str) -> Vec<String> {
        self.index_keys
            .values()
            .map(|key| Self::index_with(key, field, value))
            .collect()
    }
    fn prepare_cipher(key: &[u8]) -> Result<Aes256Gcm> {
        // Enforce the sensitive-state Drop contract in production builds too.
        fn zeroizes_on_drop<T: zeroize::ZeroizeOnDrop>(value: T) -> T {
            value
        }
        Aes256Gcm::new_from_slice(key)
            .map(zeroizes_on_drop)
            .map_err(|_| Error::Internal)
    }
    fn cipher(&self, version: i16) -> Result<&Aes256Gcm> {
        self.keys
            .get(&version)
            .ok_or(Error::PolicyGate("PAYOUT_KEY_VERSION_MISSING"))
    }
    fn aad(prefix: &[u8], subject: Uuid, header: &[u8; HEADER_LEN]) -> Vec<u8> {
        let mut aad = Vec::with_capacity(prefix.len() + 16 + HEADER_LEN);
        aad.extend_from_slice(prefix);
        aad.extend_from_slice(subject.as_bytes());
        aad.extend_from_slice(header);
        aad
    }
    pub fn seal(&self, org: Uuid, number: &str) -> Result<Vec<u8>> {
        self.seal_with(AAD_PREFIX, org, number)
    }
    pub fn open(&self, org: Uuid, sealed: &[u8]) -> Result<Zeroizing<String>> {
        self.open_with(AAD_PREFIX, org, sealed, true)
    }
    /// Seals one personal-data value for `subject` (the row's owner id).
    pub fn seal_field(&self, field: Field, subject: Uuid, value: &str) -> Result<Vec<u8>> {
        self.seal_with(field.aad_prefix(), subject, value)
    }
    pub fn open_field(
        &self,
        field: Field,
        subject: Uuid,
        sealed: &[u8],
    ) -> Result<Zeroizing<String>> {
        self.open_with(field.aad_prefix(), subject, sealed, false)
    }
    fn seal_with(&self, prefix: &[u8], subject: Uuid, number: &str) -> Result<Vec<u8>> {
        let cipher = self.cipher(self.active)?;
        let mut header = [0; HEADER_LEN];
        header[..TAG.len()].copy_from_slice(TAG);
        header[TAG.len()..].copy_from_slice(&self.active.to_be_bytes());
        let nonce = Nonce::generate();
        let capacity = number
            .len()
            .checked_add(HEADER_LEN + NONCE_LEN + AUTH_TAG_LEN)
            .ok_or(Error::Internal)?;
        // Also wipe on an encryption error, when the buffer may hold plaintext.
        let mut out = Zeroizing::new(Vec::with_capacity(capacity));
        out.extend_from_slice(&header);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(number.as_bytes());
        let tag = cipher
            .encrypt_inout_detached(
                &nonce,
                &Self::aad(prefix, subject, &header),
                (&mut out[HEADER_LEN + NONCE_LEN..]).into(),
            )
            .map_err(|_| Error::Internal)?;
        out.extend_from_slice(&tag);
        Ok(std::mem::take(&mut *out))
    }
    fn open_with(
        &self,
        prefix: &[u8],
        subject: Uuid,
        sealed: &[u8],
        allow_legacy: bool,
    ) -> Result<Zeroizing<String>> {
        let aad_storage;
        let (version, rest, aad) = if sealed.starts_with(TAG) {
            let header: &[u8; HEADER_LEN] = sealed
                .get(..HEADER_LEN)
                .ok_or(Error::Internal)?
                .try_into()
                .map_err(|_| Error::Internal)?;
            aad_storage = Self::aad(prefix, subject, header);
            (
                i16::from_be_bytes(
                    header[TAG.len()..]
                        .try_into()
                        .map_err(|_| Error::Internal)?,
                ),
                &sealed[header.len()..],
                aad_storage.as_slice(),
            )
        } else if allow_legacy {
            (1, sealed, subject.as_bytes().as_slice())
        } else {
            return Err(Error::Internal);
        };
        if rest.len() < NONCE_LEN + AUTH_TAG_LEN {
            return Err(Error::Internal);
        }
        let (nonce, ct) = rest.split_at(NONCE_LEN);
        let (ct, tag) = ct.split_at(ct.len() - AUTH_TAG_LEN);
        let mut pt = Zeroizing::new(ct.to_vec());
        self.cipher(version)?
            .decrypt_inout_detached(
                &Nonce::try_from(nonce).map_err(|_| Error::Internal)?,
                aad,
                pt.as_mut_slice().into(),
                &aes_gcm::Tag::try_from(tag).map_err(|_| Error::Internal)?,
            )
            .map_err(|_| Error::Internal)?;
        // Transfer the same allocation into String; wipe invalid UTF-8 as well.
        match String::from_utf8(std::mem::take(&mut *pt)) {
            Ok(value) => Ok(Zeroizing::new(value)),
            Err(error) => {
                let _invalid = Zeroizing::new(error.into_bytes());
                Err(Error::Internal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{Aead, Payload};
    #[test]
    fn development_can_disable_payouts_with_an_absent_or_empty_key() {
        let org = Uuid::new_v4();
        for key in [None, Some(String::new()), Some("  ".to_owned())] {
            let disabled = KeyRing::development_key(key).unwrap();
            assert!(disabled.seal(org, "11012345678901").is_err());
        }
        let enabled = KeyRing::development_key(Some("11".repeat(32))).unwrap();
        let sealed = enabled.seal(org, "11012345678901").unwrap();
        assert_eq!(&*enabled.open(org, &sealed).unwrap(), "11012345678901");
        assert!(KeyRing::development_key(Some("invalid".to_owned())).is_err());
    }
    fn ring(active: i16) -> KeyRing {
        KeyRing::parse(
            format!(
                r#"{{"version":1,"active_version":{active},"keys":{{"1":"{}","2":"{}"}}}}"#,
                "11".repeat(32),
                "22".repeat(32)
            )
            .as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn rotation_preserves_history_and_authenticates_metadata() {
        let org = Uuid::new_v4();
        let old = ring(1).seal(org, "11012345678901").unwrap();
        let rotated = ring(2);
        assert_eq!(&*rotated.open(org, &old).unwrap(), "11012345678901");
        let new = rotated.seal(org, "11012345678901").unwrap();
        assert_ne!(new, rotated.seal(org, "11012345678901").unwrap());
        assert!(rotated.open(Uuid::new_v4(), &new).is_err());
        let mut tampered = new.clone();
        tampered[TAG.len() + 1] = 1;
        assert!(rotated.open(org, &tampered).is_err());
        let mut tampered = new;
        *tampered.last_mut().unwrap() ^= 1;
        assert!(rotated.open(org, &tampered).is_err());
    }
    #[test]
    fn legacy_rows_survive_a_keyring_upgrade() {
        let org = Uuid::new_v4();
        let r = ring(2);
        let nonce = Nonce::generate();
        let ct = r
            .cipher(1)
            .unwrap()
            .encrypt(
                &nonce,
                Payload {
                    msg: b"11012345678901",
                    aad: org.as_bytes(),
                },
            )
            .unwrap();
        let mut legacy = nonce.to_vec();
        legacy.extend_from_slice(&ct);
        assert_eq!(&*r.open(org, &legacy).unwrap(), "11012345678901");
        assert!(r.open(Uuid::new_v4(), &legacy).is_err());
    }
    #[test]
    fn fields_are_bound_to_column_and_subject() {
        let r = ring(2);
        let user = Uuid::new_v4();
        let sealed = r
            .seal_field(Field::IdentityRealName, user, "홍길동")
            .unwrap();
        assert_eq!(
            &*r.open_field(Field::IdentityRealName, user, &sealed)
                .unwrap(),
            "홍길동"
        );
        assert!(
            r.open_field(Field::IdentityBirthDate, user, &sealed)
                .is_err()
        );
        assert!(
            r.open_field(Field::IdentityRealName, Uuid::new_v4(), &sealed)
                .is_err()
        );
        // An account-number ciphertext cannot be read as a field, nor the reverse.
        assert!(r.open(user, &sealed).is_err());
        let account = r.seal(user, "11012345678901").unwrap();
        assert!(r.open_field(Field::TaxTin, user, &account).is_err());
        // Header-less (legacy) ciphertexts are accepted only for account numbers.
        assert!(
            r.open_field(Field::IdentityRealName, user, &sealed[HEADER_LEN..])
                .is_err()
        );
        // Old versions stay readable after rotation.
        let old = ring(1)
            .seal_field(Field::ContactPhone, user, "010")
            .unwrap();
        assert_eq!(
            &*r.open_field(Field::ContactPhone, user, &old).unwrap(),
            "010"
        );
    }

    #[test]
    fn blind_index_keys_are_not_the_cipher_keys() {
        let r = ring(2);
        assert!(r.ready());
        assert_eq!(r.blind_indexes(Field::UserEmail, "a@b").len(), 2);
        assert_ne!(*r.index_keys[&2], [0x22; 32]);
        assert!(!KeyRing::development_key(None).unwrap().ready());
        assert!(
            KeyRing::development_key(None)
                .unwrap()
                .blind_index(Field::UserEmail, "a@b")
                .is_err()
        );
    }

    #[test]
    fn invalid_ring_fails_closed() {
        assert!(KeyRing::parse(br#"{"version":1,"active_version":2,"keys":{"1":"00"}}"#).is_err());
        assert!(KeyRing::parse(br#"{"version":1,"active_version":2,"keys":{}}"#).is_err());
    }

    #[test]
    fn in_place_ciphertext_matches_the_previous_wire_format() {
        let org = Uuid::new_v4();
        let ring = ring(2);
        let reference = Aes256Gcm::new_from_slice(&[0x22; 32]).unwrap();
        for text in [
            String::new(),
            "11012345678901".into(),
            "한글\n".repeat(17),
            "x".repeat(4096),
        ] {
            let sealed = ring.seal(org, &text).unwrap();
            let header = &sealed[..HEADER_LEN];
            let nonce = Nonce::try_from(&sealed[HEADER_LEN..HEADER_LEN + NONCE_LEN]).unwrap();
            let mut aad = AAD_PREFIX.to_vec();
            aad.extend_from_slice(org.as_bytes());
            aad.extend_from_slice(header);
            let expected = reference
                .encrypt(
                    &nonce,
                    Payload {
                        msg: text.as_bytes(),
                        aad: &aad,
                    },
                )
                .unwrap();
            assert_eq!(&sealed[HEADER_LEN + NONCE_LEN..], expected);
            assert_eq!(&*ring.open(org, &sealed).unwrap(), &text);
        }
    }

    #[test]
    fn truncation_tampering_unknown_versions_and_invalid_utf8_are_rejected() {
        let org = Uuid::new_v4();
        let ring = ring(2);
        let sealed = ring.seal(org, "11012345678901").unwrap();
        for end in 0..sealed.len() {
            assert!(ring.open(org, &sealed[..end]).is_err());
        }
        for index in 0..sealed.len() {
            let mut changed = sealed.clone();
            changed[index] ^= 1;
            assert!(ring.open(org, &changed).is_err(), "byte {index}");
        }
        let mut unknown = sealed.clone();
        unknown[TAG.len()..HEADER_LEN].copy_from_slice(&3i16.to_be_bytes());
        assert!(matches!(
            ring.open(org, &unknown),
            Err(Error::PolicyGate("PAYOUT_KEY_VERSION_MISSING"))
        ));
        let nonce = Nonce::generate();
        let ct = ring
            .cipher(1)
            .unwrap()
            .encrypt(
                &nonce,
                Payload {
                    msg: &[0xff],
                    aad: org.as_bytes(),
                },
            )
            .unwrap();
        let mut invalid = nonce.to_vec();
        invalid.extend_from_slice(&ct);
        assert!(ring.open(org, &invalid).is_err());
    }

    #[test]
    fn prepared_ciphers_support_concurrent_organizations() {
        let ring = std::sync::Arc::new(ring(2));
        let tasks: Vec<_> = (1..=4)
            .map(|i| {
                let ring = ring.clone();
                std::thread::spawn(move || {
                    let org = Uuid::from_u128(i);
                    for _ in 0..64 {
                        let sealed = ring.seal(org, "11012345678901").unwrap();
                        assert_eq!(&*ring.open(org, &sealed).unwrap(), "11012345678901");
                        assert!(ring.open(Uuid::nil(), &sealed).is_err());
                    }
                })
            })
            .collect();
        for task in tasks {
            task.join().unwrap();
        }
    }
}
