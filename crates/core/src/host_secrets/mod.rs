//! Host-side secret tooling (`audeniq-secrets`): KMS-wrapped payout keyrings
//! ([`keyring`]) and secret-manager materialization ([`store`]) for AWS, GCP
//! and OCI. Plaintext only ever lands in tmpfs; every replacement is atomic
//! and happens only after every cloud call succeeded.
pub mod keyring;
pub mod store;

use crate::cloud::{self, CloudError, Result, fail};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    Aws,
    Gcp,
    Oci,
}

impl Provider {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "aws" => Ok(Self::Aws),
            "gcp" => Ok(Self::Gcp),
            "oci" => Ok(Self::Oci),
            _ => fail("unsupported provider (aws, gcp or oci)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::Oci => "oci",
        }
    }
}

/// Every cloud operation the tool performs; [`Clouds`] is the real one,
/// tests substitute a fake.
#[async_trait::async_trait]
pub trait Cloud: Send + Sync {
    async fn kms_wrap(
        &self,
        provider: Provider,
        key_id: &str,
        version: u16,
        plaintext: &[u8],
    ) -> Result<String>;
    async fn kms_generate(&self, provider: Provider, key_id: &str, version: u16) -> Result<String>;
    async fn kms_unwrap(
        &self,
        provider: Provider,
        key_id: &str,
        version: u16,
        ciphertext: &str,
    ) -> Result<Zeroizing<Vec<u8>>>;
    async fn secret_get(
        &self,
        provider: Provider,
        id: &str,
        version: &str,
    ) -> Result<Zeroizing<Vec<u8>>>;
    async fn secret_put(&self, provider: Provider, id: &str, value: &[u8]) -> Result<()>;
}

/// KMS context for one payout key version. Cloud audit logs record it, so
/// it never holds account numbers or names.
pub fn kms_context(version: u16) -> serde_json::Value {
    serde_json::json!({"service": "audeniq", "purpose": "payout-account-key", "version": version.to_string()})
}

/// GCP additional authenticated data: the same context as canonical JSON
/// (sorted keys, no spaces) - byte-identical to bundles already wrapped.
pub fn gcp_aad(version: u16) -> String {
    format!(r#"{{"purpose":"payout-account-key","service":"audeniq","version":"{version}"}}"#)
}

/// Lazily connected clients: a bundle or manifest that uses one provider
/// never needs the others' credentials.
#[derive(Default)]
pub struct Clouds {
    aws: tokio::sync::OnceCell<cloud::aws::Client>,
    gcp: tokio::sync::OnceCell<cloud::gcp::Client>,
    oci: tokio::sync::OnceCell<cloud::oci::Client>,
}

impl Clouds {
    async fn aws(&self) -> Result<&cloud::aws::Client> {
        self.aws
            .get_or_try_init(cloud::aws::Client::from_environment)
            .await
    }
    async fn gcp(&self) -> Result<&cloud::gcp::Client> {
        self.gcp
            .get_or_try_init(cloud::gcp::Client::from_environment)
            .await
    }
    async fn oci(&self) -> Result<&cloud::oci::Client> {
        self.oci
            .get_or_try_init(|| async { cloud::oci::Client::from_environment() })
            .await
    }
}

#[async_trait::async_trait]
impl Cloud for Clouds {
    async fn kms_wrap(
        &self,
        provider: Provider,
        key_id: &str,
        version: u16,
        plaintext: &[u8],
    ) -> Result<String> {
        match provider {
            Provider::Aws => {
                self.aws()
                    .await?
                    .kms_encrypt(key_id, plaintext, &kms_context(version))
                    .await
            }
            Provider::Gcp => Ok(cloud::b64(
                &self
                    .gcp()
                    .await?
                    .kms(key_id, true, plaintext, gcp_aad(version).as_bytes())
                    .await?,
            )),
            Provider::Oci => {
                self.oci()
                    .await?
                    .kms_encrypt(key_id, plaintext, &kms_context(version))
                    .await
            }
        }
    }

    async fn kms_generate(&self, provider: Provider, key_id: &str, version: u16) -> Result<String> {
        match provider {
            Provider::Aws => {
                self.aws()
                    .await?
                    .kms_generate_without_plaintext(key_id, &kms_context(version))
                    .await
            }
            Provider::Oci => {
                self.oci()
                    .await?
                    .kms_generate_without_plaintext(key_id, &kms_context(version))
                    .await
            }
            // Cloud KMS has no data-key generation: a local random key is wrapped.
            Provider::Gcp => {
                let mut key = Zeroizing::new([0u8; 32]);
                aws_lc_rs::rand::fill(key.as_mut())
                    .map_err(|_| CloudError("random key generation failed".into()))?;
                self.kms_wrap(provider, key_id, version, key.as_ref()).await
            }
        }
    }

    async fn kms_unwrap(
        &self,
        provider: Provider,
        key_id: &str,
        version: u16,
        ciphertext: &str,
    ) -> Result<Zeroizing<Vec<u8>>> {
        match provider {
            Provider::Aws => {
                self.aws()
                    .await?
                    .kms_decrypt(key_id, ciphertext, &kms_context(version))
                    .await
            }
            Provider::Gcp => {
                let data = cloud::unb64(ciphertext, "wrapped key")?;
                self.gcp()
                    .await?
                    .kms(key_id, false, &data, gcp_aad(version).as_bytes())
                    .await
            }
            Provider::Oci => {
                self.oci()
                    .await?
                    .kms_decrypt(key_id, ciphertext, &kms_context(version))
                    .await
            }
        }
    }

    async fn secret_get(
        &self,
        provider: Provider,
        id: &str,
        version: &str,
    ) -> Result<Zeroizing<Vec<u8>>> {
        match provider {
            Provider::Aws => self.aws().await?.secret_get(id, version).await,
            Provider::Gcp => self.gcp().await?.secret_access(id, version).await,
            Provider::Oci => self.oci().await?.secret_get(id, version).await,
        }
    }

    async fn secret_put(&self, provider: Provider, id: &str, value: &[u8]) -> Result<()> {
        match provider {
            Provider::Aws => self.aws().await?.secret_put(id, value).await,
            Provider::Gcp => self.gcp().await?.secret_add_version(id, value).await,
            Provider::Oci => self.oci().await?.secret_put(id, value).await,
        }
    }
}

fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

/// A private input (0600 or stricter, owned by this user, no symlink).
pub fn private_read(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    use std::io::Read;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| CloudError("cannot open private input".into()))?;
    let m = file
        .metadata()
        .map_err(|_| CloudError("cannot stat private input".into()))?;
    if !m.is_file() || m.uid() != euid() || m.mode() & 0o077 != 0 || m.len() as usize > limit {
        return fail("input must be a private file (0600 or stricter) within the size limit");
    }
    let mut data = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| CloudError("cannot read private input".into()))?;
    if data.len() > limit {
        return fail("private input exceeds the size limit");
    }
    Ok(data)
}

fn is_tmpfs(path: &Path) -> Result<bool> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| CloudError("invalid path".into()))?;
    let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(c.as_ptr(), fs.as_mut_ptr()) } != 0 {
        return fail("cannot inspect the runtime filesystem");
    }
    // TMPFS_MAGIC; payout_keys.rs requires the same of the keyring.
    Ok(unsafe { fs.assume_init() }.f_type == 0x0102_1994)
}

/// A tmpfs directory owned by this user: 0750 with `gid` (mounted read-only
/// into the API), or 0700 without (host only).
pub fn secure_directory(path: &Path, gid: Option<u32>) -> Result<PathBuf> {
    std::fs::create_dir_all(path)
        .map_err(|_| CloudError("cannot create runtime directory".into()))?;
    let m = std::fs::symlink_metadata(path)
        .map_err(|_| CloudError("cannot stat runtime directory".into()))?;
    if m.file_type().is_symlink() || !m.is_dir() || m.uid() != euid() {
        return fail("runtime directory must be owned by this user and not a symlink");
    }
    if !is_tmpfs(path)? {
        return fail("plaintext key material must be written to tmpfs");
    }
    if let Some(gid) = gid {
        std::os::unix::fs::chown(path, Some(euid()), Some(gid))
            .map_err(|_| CloudError("cannot set runtime directory group".into()))?;
    }
    let mode = if gid.is_some() { 0o750 } else { 0o700 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|_| CloudError("cannot set runtime directory mode".into()))?;
    Ok(path.to_path_buf())
}

/// Write via a private temporary file in the same directory, fsync, rename.
/// With `gid` the file is 0640 for that group, otherwise 0600.
pub fn atomic_write(path: &Path, data: &[u8], gid: Option<u32>) -> Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .ok_or_else(|| CloudError("invalid output path".into()))?;
    let mut random = [0u8; 8];
    aws_lc_rs::rand::fill(&mut random).map_err(|_| CloudError("random name failed".into()))?;
    let tmp = dir.join(format!(".secret-{}", hex::encode(random)));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        if let Some(gid) = gid {
            std::os::unix::fs::fchown(&file, Some(euid()), Some(gid))?;
            file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
        }
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return fail("cannot write output file atomically");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod fake {
    //! A contract stub binding every wrapped key / secret to its provider,
    //! key id and context, so a wrong version or provider cannot decrypt.
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// provider, key id, context version, plaintext
    type Wrapped = (Provider, String, u16, Vec<u8>);

    #[derive(Default)]
    pub struct FakeCloud {
        pub wrapped: Mutex<HashMap<String, Wrapped>>,
        pub secrets: Mutex<HashMap<String, Vec<u8>>>,
        pub fail_provider: Mutex<Option<Provider>>,
        pub calls: Mutex<Vec<(Provider, &'static str)>>,
        pub generated: Mutex<Vec<u8>>,
    }

    impl FakeCloud {
        fn check(&self, provider: Provider, op: &'static str) -> Result<()> {
            self.calls.lock().unwrap().push((provider, op));
            if *self.fail_provider.lock().unwrap() == Some(provider) {
                return fail(format!("{} unavailable", provider.as_str()));
            }
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Cloud for FakeCloud {
        async fn kms_wrap(
            &self,
            provider: Provider,
            key_id: &str,
            version: u16,
            plaintext: &[u8],
        ) -> Result<String> {
            self.check(provider, "wrap")?;
            let mut w = self.wrapped.lock().unwrap();
            let ct = cloud::b64(format!("{}-{}", provider.as_str(), w.len()).as_bytes());
            w.insert(
                ct.clone(),
                (provider, key_id.into(), version, plaintext.to_vec()),
            );
            Ok(ct)
        }
        async fn kms_generate(
            &self,
            provider: Provider,
            key_id: &str,
            version: u16,
        ) -> Result<String> {
            let key = vec![b'G'; 32];
            *self.generated.lock().unwrap() = key.clone();
            self.check(provider, "generate")?;
            let mut w = self.wrapped.lock().unwrap();
            let ct = cloud::b64(format!("{}-{}", provider.as_str(), w.len()).as_bytes());
            w.insert(ct.clone(), (provider, key_id.into(), version, key));
            Ok(ct)
        }
        async fn kms_unwrap(
            &self,
            provider: Provider,
            key_id: &str,
            version: u16,
            ciphertext: &str,
        ) -> Result<Zeroizing<Vec<u8>>> {
            self.check(provider, "unwrap")?;
            match self.wrapped.lock().unwrap().get(ciphertext) {
                Some((p, k, v, data)) if *p == provider && k == key_id && *v == version => {
                    Ok(Zeroizing::new(data.clone()))
                }
                _ => fail("wrong key, provider or context"),
            }
        }
        async fn secret_get(
            &self,
            provider: Provider,
            id: &str,
            _version: &str,
        ) -> Result<Zeroizing<Vec<u8>>> {
            self.check(provider, "get")?;
            self.secrets
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .map(Zeroizing::new)
                .ok_or_else(|| CloudError("no such secret".into()))
        }
        async fn secret_put(&self, provider: Provider, id: &str, value: &[u8]) -> Result<()> {
            self.check(provider, "put")?;
            self.secrets
                .lock()
                .unwrap()
                .insert(id.into(), value.to_vec());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcp_aad_is_byte_identical_to_existing_bundles() {
        // json.dumps(context, sort_keys=True, separators=(',', ':')) of the
        // former Python tool, which wrapped the existing GCP keys.
        assert_eq!(
            gcp_aad(12),
            r#"{"purpose":"payout-account-key","service":"audeniq","version":"12"}"#
        );
        assert_eq!(kms_context(3)["version"], "3");
    }

    #[test]
    fn private_inputs_tmpfs_and_atomic_writes() {
        let dir = tempdir_shm();
        let path = dir.join("input");
        std::fs::write(&path, b"secret").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(private_read(&path, 64).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(private_read(&path, 64).unwrap().as_slice(), b"secret");
        assert!(private_read(&path, 3).is_err());
        let run = secure_directory(&dir.join("run"), Some(unsafe { libc::getegid() })).unwrap();
        assert_eq!(std::fs::metadata(&run).unwrap().mode() & 0o777, 0o750);
        let out = run.join("f");
        atomic_write(&out, b"one", Some(unsafe { libc::getegid() })).unwrap();
        atomic_write(&out, b"two", None).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"two");
        assert_eq!(std::fs::metadata(&out).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_dir(&run).unwrap().count(), 1);
        let disk = std::env::temp_dir().join(format!("audeniq-not-tmpfs-{}", std::process::id()));
        if !is_tmpfs(&std::env::temp_dir()).unwrap() {
            assert!(secure_directory(&disk, None).is_err());
            let _ = std::fs::remove_dir_all(&disk);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    pub(crate) fn tempdir_shm() -> PathBuf {
        let mut r = [0u8; 8];
        aws_lc_rs::rand::fill(&mut r).unwrap();
        let dir = PathBuf::from("/dev/shm").join(format!("audeniq-test-{}", hex::encode(r)));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }
}
