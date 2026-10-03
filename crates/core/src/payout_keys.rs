//! Versioned AES-256-GCM keys, separated from the database and API environment.
//! Optional KMS mode requires a read-only keyring materialized in tmpfs.
use crate::error::{Error, Result};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Generate, Payload},
};
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub const TAG: &[u8] = b"AQAC\0\x02";
pub struct KeyRing {
    active: i16,
    keys: BTreeMap<i16, Zeroizing<Vec<u8>>>,
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
    fn parse(bytes: &[u8]) -> Result<Self> {
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
        for (version, value) in &file.keys {
            let version: i16 = version
                .parse()
                .map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?;
            let value = Zeroizing::new(
                hex::decode(value).map_err(|_| Error::PolicyGate("PAYOUT_KEYRING_INVALID"))?,
            );
            if version <= 0 || value.len() != 32 || keys.insert(version, value).is_some() {
                return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
            }
        }
        if !keys.contains_key(&file.active_version) {
            return Err(Error::PolicyGate("PAYOUT_KEYRING_INVALID"));
        }
        Ok(Self {
            active: file.active_version,
            keys,
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
        let keys = match key {
            Some(key) if !key.trim().is_empty() => {
                let key = Zeroizing::new(key);
                let decoded = Zeroizing::new(
                    hex::decode(key.trim())
                        .map_err(|_| Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"))?,
                );
                if decoded.len() != 32 {
                    return Err(Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"));
                }
                BTreeMap::from([(1, decoded)])
            }
            _ => BTreeMap::new(),
        };
        Ok(Self { active: 1, keys })
    }

    pub fn active_version(&self) -> i16 {
        self.active
    }
    fn cipher(&self, version: i16) -> Result<Aes256Gcm> {
        Aes256Gcm::new_from_slice(
            self.keys
                .get(&version)
                .ok_or(Error::PolicyGate("PAYOUT_KEY_VERSION_MISSING"))?,
        )
        .map_err(|_| Error::Internal)
    }
    fn aad(org: Uuid, header: &[u8]) -> Vec<u8> {
        let mut aad = b"audeniq:portal.payout_accounts:account_number:".to_vec();
        aad.extend_from_slice(org.as_bytes());
        aad.extend_from_slice(header);
        aad
    }
    pub fn seal(&self, org: Uuid, number: &str) -> Result<Vec<u8>> {
        let mut out = TAG.to_vec();
        out.extend_from_slice(&self.active.to_be_bytes());
        let nonce = Nonce::generate();
        let ct = self
            .cipher(self.active)?
            .encrypt(
                &nonce,
                Payload {
                    msg: number.as_bytes(),
                    aad: &Self::aad(org, &out),
                },
            )
            .map_err(|_| Error::Internal)?;
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }
    pub fn open(&self, org: Uuid, sealed: &[u8]) -> Result<Zeroizing<String>> {
        let (version, rest, aad) = if sealed.starts_with(TAG) {
            let header = sealed.get(..TAG.len() + 2).ok_or(Error::Internal)?;
            (
                i16::from_be_bytes(
                    header[TAG.len()..]
                        .try_into()
                        .map_err(|_| Error::Internal)?,
                ),
                &sealed[header.len()..],
                Self::aad(org, header),
            )
        } else {
            (1, sealed, org.as_bytes().to_vec())
        };
        if rest.len() < 28 {
            return Err(Error::Internal);
        }
        let (nonce, ct) = rest.split_at(12);
        let pt = self
            .cipher(version)?
            .decrypt(
                &Nonce::try_from(nonce).map_err(|_| Error::Internal)?,
                Payload { msg: ct, aad: &aad },
            )
            .map_err(|_| Error::Internal)?;
        // Keep decrypted data in zeroizing memory, including invalid UTF-8 paths.
        let pt = Zeroizing::new(pt);
        let value = std::str::from_utf8(&pt).map_err(|_| Error::Internal)?;
        Ok(Zeroizing::new(value.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn invalid_ring_fails_closed() {
        assert!(KeyRing::parse(br#"{"version":1,"active_version":2,"keys":{"1":"00"}}"#).is_err());
        assert!(KeyRing::parse(br#"{"version":1,"active_version":2,"keys":{}}"#).is_err());
    }
}
