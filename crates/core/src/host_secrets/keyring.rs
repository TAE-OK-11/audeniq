//! KMS-wrapped payout keyring: the persistent bundle holds each AES-256
//! account key version wrapped by AWS KMS, GCP Cloud KMS or OCI Vault; the
//! API reads only the plaintext keyring materialized into tmpfs.
//!
//! Bundle format (unchanged from the former Python tool, so existing bundles
//! keep working): `{"version": 2, "active_version": N, "keys": {"N":
//! {"provider", "key_id", "ciphertext"}}}`; version 1 bundles are AWS-only
//! without `provider` and are upgraded on the next write.
use super::{Cloud, Provider, atomic_write};
use crate::cloud::{CloudError, Result, fail};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use zeroize::Zeroizing;

const MAX_VERSIONS: usize = 64;
const MAX_VERSION: u16 = 32767;
const MAX_BUNDLE: usize = 65536;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub provider: Provider,
    pub key_id: String,
    pub ciphertext: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bundle {
    pub active_version: u16,
    pub keys: BTreeMap<u16, Entry>,
}

pub fn validate_key_id(provider: Provider, key_id: &str) -> Result<()> {
    match provider {
        Provider::Aws => crate::cloud::aws::kms_key_region(key_id).map(drop),
        Provider::Gcp => crate::cloud::gcp::crypto_key_name(key_id).map(drop),
        Provider::Oci => crate::cloud::oci::key_id(key_id).map(drop),
    }
}

fn invalid<T>(what: &str) -> Result<T> {
    fail(format!("invalid wrapped keyring: {what}"))
}

impl Bundle {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BUNDLE {
            return invalid("exceeds 64 KiB");
        }
        let v: Value = serde_json::from_slice(bytes)
            .map_err(|_| CloudError("invalid wrapped keyring: not JSON".into()))?;
        let Some(o) = v.as_object() else {
            return invalid("format");
        };
        if o.len() != 3
            || !o.contains_key("version")
            || !o.contains_key("active_version")
            || !o.contains_key("keys")
        {
            return invalid("format");
        }
        let format = match o["version"].as_u64() {
            Some(f @ (1 | 2)) if o["version"].is_u64() => f,
            _ => return invalid("format version"),
        };
        let Some(keys) = o["keys"].as_object() else {
            return invalid("keys");
        };
        if keys.is_empty() || keys.len() > MAX_VERSIONS {
            return invalid("missing active key or too many versions");
        }
        let mut bundle = Bundle::default();
        for (version, key) in keys {
            let ok = !version.is_empty()
                && version.len() <= 5
                && !version.starts_with('0')
                && version.chars().all(|c| c.is_ascii_digit());
            let number: u16 = match version.parse() {
                Ok(n) if ok && n <= MAX_VERSION => n,
                _ => return invalid("key version"),
            };
            let Some(k) = key.as_object() else {
                return invalid("key fields");
            };
            let expected: &[&str] = if format == 1 {
                &["key_id", "ciphertext"]
            } else {
                &["provider", "key_id", "ciphertext"]
            };
            if k.len() != expected.len()
                || !expected
                    .iter()
                    .all(|f| k.get(*f).is_some_and(Value::is_string))
            {
                return invalid("key fields");
            }
            let provider = if format == 1 {
                Provider::Aws
            } else {
                Provider::parse(k["provider"].as_str().unwrap_or_default())?
            };
            let key_id = k["key_id"].as_str().unwrap_or_default().to_string();
            validate_key_id(provider, &key_id)?;
            let ciphertext = k["ciphertext"].as_str().unwrap_or_default().to_string();
            let raw = crate::cloud::unb64(&ciphertext, "wrapped keyring")?;
            if raw.is_empty() || raw.len() > 6144 {
                return invalid("KMS ciphertext");
            }
            bundle.keys.insert(
                number,
                Entry {
                    provider,
                    key_id,
                    ciphertext,
                },
            );
        }
        bundle.active_version = match o["active_version"].as_u64() {
            Some(a)
                if o["active_version"].is_u64()
                    && a <= u64::from(MAX_VERSION)
                    && bundle.keys.contains_key(&(a as u16)) =>
            {
                a as u16
            }
            _ => return invalid("missing active key"),
        };
        Ok(bundle)
    }

    pub fn to_json(&self) -> Result<Vec<u8>> {
        if self.keys.is_empty()
            || !self.keys.contains_key(&self.active_version)
            || self.keys.len() > MAX_VERSIONS
        {
            return invalid("missing active key or too many versions");
        }
        let keys: Map<String, Value> = self
            .keys
            .iter()
            .map(|(v, e)| (v.to_string(), json!({"provider": e.provider.as_str(), "key_id": e.key_id, "ciphertext": e.ciphertext})))
            .collect();
        let bytes = serde_json::to_vec(
            &json!({"version": 2, "active_version": self.active_version, "keys": keys}),
        )
        .map_err(|_| CloudError("cannot encode keyring".into()))?;
        // Round-trip through the parser: never persist a bundle it would refuse.
        Self::parse(&bytes)?;
        Ok(bytes)
    }

    fn next_version(&self) -> Result<u16> {
        let next = self.keys.keys().max().copied().unwrap_or(0) + 1;
        if next > MAX_VERSION || self.keys.len() >= MAX_VERSIONS {
            return fail("keyring version limit reached");
        }
        Ok(next)
    }
}

/// Import the existing 64-hex `PAYOUT_ACCOUNT_KEY` as version 1 of an empty bundle.
pub async fn wrap_legacy(
    cloud: &dyn Cloud,
    mut bundle: Bundle,
    provider: Provider,
    key_id: &str,
    legacy_hex: &[u8],
) -> Result<Bundle> {
    validate_key_id(provider, key_id)?;
    if !bundle.keys.is_empty() {
        return fail("legacy import requires an empty bundle");
    }
    let text = std::str::from_utf8(legacy_hex)
        .map_err(|_| CloudError("legacy key must be 64 hex characters".into()))?;
    let key = Zeroizing::new(
        hex::decode(text.trim())
            .map_err(|_| CloudError("legacy key must be 64 hex characters".into()))?,
    );
    if key.len() != 32 {
        return fail("legacy key must be 32 bytes");
    }
    let ciphertext = cloud.kms_wrap(provider, key_id, 1, &key).await?;
    bundle.keys.insert(
        1,
        Entry {
            provider,
            key_id: key_id.into(),
            ciphertext,
        },
    );
    bundle.active_version = 1;
    Ok(bundle)
}

/// Add a new key version (generated inside the KMS where it can) and make it active.
pub async fn generate(
    cloud: &dyn Cloud,
    mut bundle: Bundle,
    provider: Provider,
    key_id: &str,
) -> Result<Bundle> {
    validate_key_id(provider, key_id)?;
    let version = bundle.next_version()?;
    let ciphertext = cloud.kms_generate(provider, key_id, version).await?;
    bundle.keys.insert(
        version,
        Entry {
            provider,
            key_id: key_id.into(),
            ciphertext,
        },
    );
    bundle.active_version = version;
    Ok(bundle)
}

/// Run `op` for every key version with at most `concurrency` in flight;
/// every task finishes before the first failure is returned.
async fn each_version<T: Send + 'static>(
    bundle: &Bundle,
    concurrency: usize,
    cloud: Arc<dyn Cloud>,
    op: impl Fn(
        Arc<dyn Cloud>,
        u16,
        Entry,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
) -> Result<BTreeMap<u16, T>> {
    if !(1..=4).contains(&concurrency) {
        return fail("KMS concurrency must be 1..4");
    }
    let limit = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut tasks = tokio::task::JoinSet::new();
    for (&version, entry) in &bundle.keys {
        let fut = op(cloud.clone(), version, entry.clone());
        let limit = limit.clone();
        tasks.spawn(async move {
            let _permit = limit.acquire_owned().await.expect("semaphore open");
            (version, fut.await)
        });
    }
    let mut out = BTreeMap::new();
    let mut first_error = None;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((version, Ok(value))) => {
                out.insert(version, value);
            }
            Ok((_, Err(e))) => {
                first_error.get_or_insert(e);
            }
            Err(_) => {
                first_error.get_or_insert(CloudError("KMS task failed".into()));
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// Move every version to another KMS key (or provider) keeping each AES key
/// and version: no database re-encryption is needed.
pub async fn rewrap(
    cloud: Arc<dyn Cloud>,
    bundle: &Bundle,
    provider: Provider,
    key_id: &str,
    concurrency: usize,
) -> Result<Bundle> {
    validate_key_id(provider, key_id)?;
    if bundle.keys.is_empty() {
        return fail("nothing to rewrap");
    }
    let target = key_id.to_string();
    let wrapped = each_version(bundle, concurrency, cloud, move |cloud, version, entry| {
        let target = target.clone();
        Box::pin(async move {
            let key = cloud
                .kms_unwrap(entry.provider, &entry.key_id, version, &entry.ciphertext)
                .await?;
            if key.len() != 32 {
                return fail("KMS key is not AES-256");
            }
            cloud.kms_wrap(provider, &target, version, &key).await
        })
    })
    .await?;
    let keys = wrapped
        .into_iter()
        .map(|(v, ciphertext)| {
            (
                v,
                Entry {
                    provider,
                    key_id: key_id.into(),
                    ciphertext,
                },
            )
        })
        .collect();
    Ok(Bundle {
        active_version: bundle.active_version,
        keys,
    })
}

/// Decrypt every version and atomically replace the API's tmpfs keyring
/// (`{"version":1,"active_version":N,"keys":{"N":"<64 hex>"}}`, 0640 for
/// `gid`). Any failure leaves the previous keyring in place.
pub async fn materialize(
    cloud: Arc<dyn Cloud>,
    bundle: &Bundle,
    destination: &Path,
    gid: u32,
    concurrency: usize,
) -> Result<()> {
    if bundle.keys.is_empty() {
        return fail("the wrapped keyring has no keys");
    }
    let keys = each_version(bundle, concurrency, cloud, |cloud, version, entry| {
        Box::pin(async move {
            let key = cloud
                .kms_unwrap(entry.provider, &entry.key_id, version, &entry.ciphertext)
                .await?;
            if key.len() != 32 {
                return fail("KMS key is not AES-256");
            }
            Ok(Zeroizing::new(hex::encode(key.as_slice())))
        })
    })
    .await?;
    let map: Map<String, Value> = keys
        .iter()
        .map(|(v, k)| (v.to_string(), Value::String(k.to_string())))
        .collect();
    let out = Zeroizing::new(
        serde_json::to_vec(
            &json!({"version": 1, "active_version": bundle.active_version, "keys": map}),
        )
        .map_err(|_| CloudError("cannot encode keyring".into()))?,
    );
    atomic_write(destination, &out, Some(gid))
}

#[cfg(test)]
mod tests {
    use super::super::fake::FakeCloud;
    use super::super::tests::tempdir_shm;
    use super::*;

    const ARN: &str =
        "arn:aws:kms:ap-northeast-1:123456789012:key/12345678-1234-1234-1234-123456789abc";
    const GCP: &str =
        "projects/audeniq-prod/locations/asia-northeast3/keyRings/backend/cryptoKeys/payout";
    const OCI: &str = "bbqexamplevault-crypto.kms.ap-seoul-1.oraclecloud.com/ocid1.key.oc1.ap-seoul-1.bbqexamplevault.abuwcljrexamplekey0123456789";

    fn fake() -> Arc<FakeCloud> {
        Arc::new(FakeCloud::default())
    }

    async fn legacy_v1(cloud: &FakeCloud) -> Vec<u8> {
        let ct = cloud
            .kms_wrap(Provider::Aws, ARN, 1, &[b'L'; 32])
            .await
            .unwrap();
        serde_json::to_vec(&json!({"version": 1, "active_version": 1, "keys": {"1": {"key_id": ARN, "ciphertext": ct}}})).unwrap()
    }

    async fn materialized(cloud: Arc<FakeCloud>, bundle: &Bundle) -> Value {
        let dir = tempdir_shm();
        let out = dir.join("payout-keyring.json");
        materialize(cloud, bundle, &out, unsafe { libc::getegid() }, 2)
            .await
            .unwrap();
        let v = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        v
    }

    #[tokio::test]
    async fn legacy_aws_bundle_reads_and_mixes_with_every_provider() {
        let cloud = fake();
        let mut bundle = Bundle::parse(&legacy_v1(&cloud).await).unwrap();
        assert_eq!(bundle.keys[&1].provider, Provider::Aws);
        bundle = generate(cloud.as_ref(), bundle, Provider::Gcp, GCP)
            .await
            .unwrap();
        bundle = generate(cloud.as_ref(), bundle, Provider::Oci, OCI)
            .await
            .unwrap();
        assert_eq!(bundle.active_version, 3);
        let reparsed = Bundle::parse(&bundle.to_json().unwrap()).unwrap();
        assert_eq!(reparsed, bundle);
        let v = materialized(cloud.clone(), &bundle).await;
        assert_eq!(v["active_version"], 3);
        assert_eq!(v["keys"]["1"], hex::encode([b'L'; 32]));
        assert_eq!(v["keys"]["3"], hex::encode([b'G'; 32]));
        // The produced file is exactly what the API accepts.
        let parsed = crate::payout_keys::KeyRing::parse(&serde_json::to_vec(&v).unwrap());
        assert!(parsed.is_ok());
    }

    #[tokio::test]
    async fn rewrap_between_all_providers_keeps_material_and_versions() {
        let cloud = fake();
        let source = Bundle::parse(&legacy_v1(&cloud).await).unwrap();
        let mut bundle = source.clone();
        for (provider, key) in [
            (Provider::Gcp, GCP),
            (Provider::Oci, OCI),
            (Provider::Aws, ARN),
        ] {
            bundle = rewrap(cloud.clone(), &bundle, provider, key, 2)
                .await
                .unwrap();
            assert_eq!(bundle.keys[&1].provider, provider);
            assert_eq!(bundle.active_version, 1);
            assert_eq!(
                cloud
                    .kms_unwrap(provider, key, 1, &bundle.keys[&1].ciphertext)
                    .await
                    .unwrap()
                    .as_slice(),
                &[b'L'; 32]
            );
        }
        assert_eq!(source.keys[&1].provider, Provider::Aws);
    }

    #[tokio::test]
    async fn context_version_mismatch_refuses_decryption() {
        let cloud = fake();
        for (provider, key) in [
            (Provider::Aws, ARN),
            (Provider::Gcp, GCP),
            (Provider::Oci, OCI),
        ] {
            let ct = cloud.kms_wrap(provider, key, 1, &[b'K'; 32]).await.unwrap();
            assert!(cloud.kms_unwrap(provider, key, 2, &ct).await.is_err());
        }
    }

    #[tokio::test]
    async fn failure_in_one_provider_keeps_the_previous_runtime_keyring() {
        let cloud = fake();
        let mut bundle = Bundle::parse(&legacy_v1(&cloud).await).unwrap();
        bundle = generate(cloud.as_ref(), bundle, Provider::Oci, OCI)
            .await
            .unwrap();
        *cloud.fail_provider.lock().unwrap() = Some(Provider::Oci);
        let dir = tempdir_shm();
        let out = dir.join("payout-keyring.json");
        std::fs::write(&out, b"previous keyring").unwrap();
        assert!(
            materialize(cloud.clone(), &bundle, &out, unsafe { libc::getegid() }, 2)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&out).unwrap(), b"previous keyring");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(
            rewrap(cloud.clone(), &bundle, Provider::Gcp, GCP, 1)
                .await
                .is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn legacy_import_and_limits() {
        let cloud = fake();
        let b = wrap_legacy(
            cloud.as_ref(),
            Bundle::default(),
            Provider::Oci,
            OCI,
            format!("{}\n", "ab".repeat(32)).as_bytes(),
        )
        .await
        .unwrap();
        assert_eq!(b.active_version, 1);
        assert!(
            wrap_legacy(
                cloud.as_ref(),
                b.clone(),
                Provider::Oci,
                OCI,
                "ab".repeat(32).as_bytes()
            )
            .await
            .is_err()
        );
        assert!(
            wrap_legacy(
                cloud.as_ref(),
                Bundle::default(),
                Provider::Oci,
                OCI,
                b"abcd"
            )
            .await
            .is_err()
        );
        assert!(
            each_version(&b, 5, cloud.clone(), |_, _, _| Box::pin(async { Ok(()) }))
                .await
                .is_err()
        );
    }

    #[test]
    fn invalid_bundles_refuse_before_any_cloud_call() {
        let entry = json!({"provider": "gcp", "key_id": GCP, "ciphertext": "YQ=="});
        let bad = [
            json!({"version": 2, "active_version": 1, "keys": {"1": {"provider": "other", "key_id": GCP, "ciphertext": "YQ=="}}}),
            json!({"version": 2, "active_version": 1, "keys": {"1": {"provider": "gcp", "key_id": GCP, "ciphertext": "YQ==", "plaintext": "11"}}}),
            json!({"version": 2, "active_version": 1, "keys": {"1": {"provider": "aws", "key_id": GCP, "ciphertext": "YQ=="}}}),
            json!({"version": 1, "active_version": 1, "keys": {"1": entry}}),
            json!({"version": 2, "active_version": 2, "keys": {"1": entry}}),
            json!({"version": 2, "active_version": 1, "keys": {"01": entry}}),
            json!({"version": 3, "active_version": 1, "keys": {"1": entry}}),
            json!({"version": 2, "active_version": 1, "keys": {}}),
            json!({"version": 2, "active_version": 1, "keys": {"1": {"provider": "gcp", "key_id": GCP, "ciphertext": "not base64!"}}}),
        ];
        for b in bad {
            assert!(
                Bundle::parse(&serde_json::to_vec(&b).unwrap()).is_err(),
                "{b}"
            );
        }
        assert!(
            Bundle::parse(
                &serde_json::to_vec(
                    &json!({"version": 2, "active_version": 1, "keys": {"1": entry}})
                )
                .unwrap()
            )
            .is_ok()
        );
    }
}
