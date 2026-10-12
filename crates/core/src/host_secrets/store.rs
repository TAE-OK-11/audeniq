//! Secret-manager materialization: a private manifest maps each compose
//! variable (DB passwords, service secrets, storage keys…) or runtime key
//! file to one pinned secret in AWS Secrets Manager, GCP Secret Manager or
//! OCI Vault. `materialize` writes `secrets.env` (host-only tmpfs, read by
//! deploy.sh as a second compose `--env-file`) and the key files (the
//! runtime key directory the API mounts read-only).
//!
//! ```json
//! {"version": 1, "secrets": [
//!   {"env": "POSTGRES_PASSWORD", "provider": "aws",
//!    "secret_id": "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-AbCdEf",
//!    "json_key": "password"},
//!   {"env": "EDGE_SERVICE_SECRET", "provider": "gcp", "secret_id": "projects/audeniq-prod/secrets/edge"},
//!   {"file": "payout-keyring.json", "provider": "oci",
//!    "secret_id": "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaa.example"}
//! ]}
//! ```
use super::{Cloud, Provider, atomic_write};
use crate::cloud::{CloudError, Result, fail};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use zeroize::Zeroizing;

pub const MAX_VALUE: usize = 65536;
pub const ENV_FILE: &str = "secrets.env";
/// deploy.sh owns this line of production.env; a secret must never replace it.
const RESERVED_ENV: &[&str] = &["AUDENIQ_IMAGE"];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Env(String),
    File(String),
}

#[derive(Clone, Debug)]
pub struct Secret {
    pub target: Target,
    pub provider: Provider,
    pub secret_id: String,
    pub json_key: Option<String>,
    pub version: String,
}

fn env_name_ok(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !RESERVED_ENV.contains(&s)
}

fn file_name_ok(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        && s != ENV_FILE
}

pub fn validate_secret_id(provider: Provider, id: &str) -> Result<()> {
    match provider {
        Provider::Aws => crate::cloud::aws::secret_region(id).map(drop),
        Provider::Gcp => crate::cloud::gcp::secret_name(id).map(drop),
        Provider::Oci => crate::cloud::oci::secret_id(id).map(drop),
    }
}

fn version_ok(provider: Provider, v: &str) -> bool {
    match provider {
        // Version stage, e.g. AWSCURRENT / AWSPREVIOUS.
        Provider::Aws => {
            (1..=256).contains(&v.len())
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }
        // Version number or latest (placed in the request path).
        Provider::Gcp => {
            v == "latest"
                || (v.len() <= 10
                    && !v.starts_with('0')
                    && !v.is_empty()
                    && v.chars().all(|c| c.is_ascii_digit()))
        }
        Provider::Oci => matches!(v, "CURRENT" | "PENDING" | "PREVIOUS" | "LATEST"),
    }
}

fn default_version(provider: Provider) -> &'static str {
    match provider {
        Provider::Aws => "AWSCURRENT",
        Provider::Gcp => "latest",
        Provider::Oci => "CURRENT",
    }
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Vec<Secret>> {
    let bad = |what: &str| CloudError(format!("invalid secret manifest: {what}"));
    let v: Value = serde_json::from_slice(bytes).map_err(|_| bad("not JSON"))?;
    let o = v.as_object().ok_or_else(|| bad("format"))?;
    if o.len() != 2 || o.get("version").and_then(Value::as_u64) != Some(1) || !o["version"].is_u64()
    {
        return Err(bad("format"));
    }
    let entries = o
        .get("secrets")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("secrets"))?;
    if entries.is_empty() || entries.len() > 128 {
        return Err(bad("a manifest lists 1..128 secrets"));
    }
    let mut targets = HashSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let e = e.as_object().ok_or_else(|| bad("entry"))?;
        const FIELDS: &[&str] = &[
            "env",
            "file",
            "provider",
            "secret_id",
            "json_key",
            "version",
        ];
        if !e.keys().all(|k| FIELDS.contains(&k.as_str())) || !e.values().all(Value::is_string) {
            return Err(bad("entry fields"));
        }
        let s = |k: &str| e.get(k).and_then(Value::as_str);
        let target = match (s("env"), s("file")) {
            (Some(env), None) if env_name_ok(env) => Target::Env(env.into()),
            (None, Some(file)) if file_name_ok(file) => Target::File(file.into()),
            (Some(_), Some(_)) | (None, None) => {
                return Err(bad("each secret has exactly one target: env or file"));
            }
            _ => return Err(bad("invalid or reserved target name")),
        };
        if !targets.insert(target.clone()) {
            return Err(bad("duplicate target"));
        }
        let provider = Provider::parse(s("provider").unwrap_or_default())?;
        let secret_id = s("secret_id").unwrap_or_default().to_string();
        validate_secret_id(provider, &secret_id)?;
        let json_key = s("json_key").map(str::to_string);
        if json_key.as_deref().is_some_and(|k| {
            !(1..=128).contains(&k.len())
                || !k
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
        }) {
            return Err(bad("json_key"));
        }
        let version = s("version")
            .unwrap_or(default_version(provider))
            .to_string();
        if !version_ok(provider, &version) {
            return Err(bad("version for provider"));
        }
        out.push(Secret {
            target,
            provider,
            secret_id,
            json_key,
            version,
        });
    }
    Ok(out)
}

/// One `KEY='value'` line: compose reads single-quoted values literally
/// (`$`, `#`, `"`, spaces). A quote or line break could not round-trip.
fn env_value(raw: &[u8]) -> Result<Zeroizing<String>> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| CloudError("an environment secret must be UTF-8".into()))?;
    if text.is_empty() || text.contains(['\'', '\n', '\r', '\0']) {
        return fail("an environment secret must be one non-empty line without single quotes");
    }
    Ok(Zeroizing::new(text.to_string()))
}

async fn resolve(cloud: &dyn Cloud, s: &Secret) -> Result<Zeroizing<Vec<u8>>> {
    let raw = cloud
        .secret_get(s.provider, &s.secret_id, &s.version)
        .await?;
    if raw.len() > MAX_VALUE {
        return fail("secret value exceeds limit");
    }
    let Some(key) = &s.json_key else {
        return Ok(raw);
    };
    let doc: Option<Value> = serde_json::from_slice(&raw).ok();
    let value = match doc.as_ref().and_then(|d| d.get(key)) {
        Some(Value::String(v)) => v.clone(),
        Some(v @ Value::Number(_)) => v.to_string(),
        _ => return fail("secret has no such JSON string or number field"),
    };
    Ok(Zeroizing::new(value.into_bytes()))
}

/// Read every secret first; only then replace the key files and secrets.env.
pub async fn materialize(
    cloud: &dyn Cloud,
    secrets: &[Secret],
    key_dir: &Path,
    env_dir: &Path,
    gid: u32,
) -> Result<(usize, usize)> {
    let mut env = Zeroizing::new(String::new());
    let mut files = Vec::new();
    for s in secrets {
        let value = resolve(cloud, s).await?;
        match &s.target {
            Target::Env(name) => {
                let v = env_value(&value)?;
                env.push_str(&format!("{name}='{}'\n", v.as_str()));
            }
            Target::File(name) => files.push((name.clone(), value)),
        }
    }
    for (name, value) in &files {
        atomic_write(&key_dir.join(name), value, Some(gid))?;
    }
    // Host only: owner 0600 in a directory no container mounts.
    atomic_write(&env_dir.join(ENV_FILE), env.as_bytes(), None)?;
    Ok((secrets.len() - files.len(), files.len()))
}

/// Store a new version of an existing secret from a private value file.
pub async fn put(cloud: &dyn Cloud, secrets: &[Secret], target: &str, value: &[u8]) -> Result<()> {
    let s = secrets
        .iter()
        .find(|s| matches!(&s.target, Target::Env(n) | Target::File(n) if n == target))
        .ok_or_else(|| CloudError("target is not in the manifest".into()))?;
    if s.json_key.is_some() {
        return fail("a JSON secret is updated as a whole in its provider");
    }
    if value.len() > MAX_VALUE {
        return fail("secret value exceeds limit");
    }
    let value: Zeroizing<Vec<u8>> = match s.target {
        Target::Env(_) => {
            // A trailing newline from an editor is not part of the value.
            let trimmed = value.strip_suffix(b"\n").unwrap_or(value);
            Zeroizing::new(env_value(trimmed)?.as_bytes().to_vec())
        }
        Target::File(_) => Zeroizing::new(value.to_vec()),
    };
    cloud.secret_put(s.provider, &s.secret_id, &value).await
}

#[cfg(test)]
mod tests {
    use super::super::fake::FakeCloud;
    use super::super::tests::tempdir_shm;
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::MetadataExt;

    const AWS: &str = "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-AbCdEf";
    const GCP: &str = "projects/audeniq-prod/secrets/edge-service-secret";
    const OCI: &str = "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample.secret0123";

    fn manifest(entries: Value) -> Result<Vec<Secret>> {
        parse_manifest(&serde_json::to_vec(&json!({"version": 1, "secrets": entries})).unwrap())
    }

    #[test]
    fn manifests_pin_ids_and_targets() {
        let ok = manifest(json!([
            {"env": "POSTGRES_PASSWORD", "provider": "aws", "secret_id": AWS, "json_key": "password"},
            {"env": "EDGE_SERVICE_SECRET", "provider": "gcp", "secret_id": GCP, "version": "3"},
            {"file": "payout-keyring.json", "provider": "oci", "secret_id": OCI, "version": "CURRENT"},
        ]))
        .unwrap();
        assert_eq!(ok[0].version, "AWSCURRENT");
        let bad = [
            json!({"env": "A", "provider": "aws", "secret_id": "audeniq/db"}),
            json!({"env": "A", "provider": "aws", "secret_id": AWS.replace("secretsmanager", "kms")}),
            json!({"env": "A", "provider": "gcp", "secret_id": format!("{GCP}/versions/1")}),
            json!({"env": "A", "provider": "oci", "secret_id": OCI.replace("vaultsecret", "key")}),
            json!({"env": "A", "provider": "azure", "secret_id": AWS}),
            json!({"env": "AUDENIQ_IMAGE", "provider": "aws", "secret_id": AWS}),
            json!({"env": "lower", "provider": "aws", "secret_id": AWS}),
            json!({"env": "A", "file": "x", "provider": "aws", "secret_id": AWS}),
            json!({"provider": "aws", "secret_id": AWS}),
            json!({"file": "../escape", "provider": "aws", "secret_id": AWS}),
            json!({"file": "secrets.env", "provider": "aws", "secret_id": AWS}),
            json!({"env": "A", "provider": "gcp", "secret_id": GCP, "version": "AWSCURRENT"}),
            json!({"env": "A", "provider": "gcp", "secret_id": GCP, "version": "1/../x"}),
            json!({"env": "A", "provider": "aws", "secret_id": AWS, "extra": "1"}),
        ];
        for b in bad {
            assert!(manifest(json!([b])).is_err(), "{b}");
        }
        let dup = json!({"env": "A", "provider": "aws", "secret_id": AWS});
        assert!(manifest(json!([dup, {"env": "A", "provider": "gcp", "secret_id": GCP}])).is_err());
    }

    fn cloud_with_values() -> FakeCloud {
        let cloud = FakeCloud::default();
        let mut s = cloud.secrets.lock().unwrap();
        s.insert(AWS.into(), serde_json::to_vec(&json!({"host": "10.0.0.5", "username": "audeniq_owner", "password": "p$w\"d#x y", "port": 5432})).unwrap());
        s.insert(GCP.into(), b"edge-secret-value".to_vec());
        s.insert(OCI.into(), b"{\"version\":1}".to_vec());
        drop(s);
        cloud
    }

    #[tokio::test]
    async fn env_file_is_literal_and_key_files_are_group_readable() {
        let cloud = cloud_with_values();
        let secrets = manifest(json!([
            {"env": "DB_HOST", "provider": "aws", "secret_id": AWS, "json_key": "host"},
            {"env": "DB_PORT", "provider": "aws", "secret_id": AWS, "json_key": "port"},
            {"env": "POSTGRES_PASSWORD", "provider": "aws", "secret_id": AWS, "json_key": "password"},
            {"env": "EDGE_SERVICE_SECRET", "provider": "gcp", "secret_id": GCP},
            {"file": "payout-keyring.json", "provider": "oci", "secret_id": OCI},
        ]))
        .unwrap();
        let dir = tempdir_shm();
        let (keys, env) = (dir.join("keys"), dir.join("env"));
        std::fs::create_dir(&keys).unwrap();
        std::fs::create_dir(&env).unwrap();
        let gid = unsafe { libc::getegid() };
        assert_eq!(
            materialize(&cloud, &secrets, &keys, &env, gid)
                .await
                .unwrap(),
            (4, 1)
        );
        assert_eq!(
            std::fs::read_to_string(env.join(ENV_FILE)).unwrap(),
            "DB_HOST='10.0.0.5'\nDB_PORT='5432'\nPOSTGRES_PASSWORD='p$w\"d#x y'\nEDGE_SERVICE_SECRET='edge-secret-value'\n"
        );
        assert_eq!(
            std::fs::metadata(env.join(ENV_FILE)).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read(keys.join("payout-keyring.json")).unwrap(),
            b"{\"version\":1}"
        );
        assert_eq!(
            std::fs::metadata(keys.join("payout-keyring.json"))
                .unwrap()
                .mode()
                & 0o777,
            0o640
        );
        if let Ok(out) = std::process::Command::new("docker")
            .arg("compose")
            .arg("version")
            .output()
            && out.status.success()
        {
            let compose = dir.join("c.yaml");
            std::fs::write(&compose, "services:\n  x:\n    image: busybox\n    environment:\n      P: ${POSTGRES_PASSWORD}\n").unwrap();
            let out = std::process::Command::new("docker")
                .args(["compose", "-f"])
                .arg(&compose)
                .arg("--env-file")
                .arg(env.join(ENV_FILE))
                .args(["config", "--format", "json"])
                .output()
                .unwrap();
            if out.status.success() {
                let v: Value = serde_json::from_slice(&out.stdout).unwrap();
                // `config` re-escapes a literal $ as $$: no interpolation happened.
                assert_eq!(v["services"]["x"]["environment"]["P"], "p$$w\"d#x y");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn one_failed_secret_replaces_nothing() {
        let cloud = cloud_with_values();
        let secrets = manifest(json!([
            {"env": "A", "provider": "aws", "secret_id": AWS, "json_key": "password"},
            {"file": "k.json", "provider": "oci", "secret_id": OCI},
            {"env": "B", "provider": "gcp", "secret_id": GCP},
        ]))
        .unwrap();
        *cloud.fail_provider.lock().unwrap() = Some(Provider::Gcp);
        let dir = tempdir_shm();
        std::fs::write(dir.join(ENV_FILE), "previous").unwrap();
        assert!(
            materialize(&cloud, &secrets, &dir, &dir, unsafe { libc::getegid() })
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(ENV_FILE)).unwrap(),
            "previous"
        );
        assert!(!dir.join("k.json").exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn unrepresentable_values_refuse() {
        let dir = tempdir_shm();
        for (value, key) in [
            (&b"has'quote"[..], None),
            (b"two\nlines", None),
            (b"", None),
            (b"not json", Some("password")),
            (b"{\"password\": {\"nested\": 1}}", Some("password")),
            (b"{\"password\": true}", Some("password")),
            (b"{\"user\": \"x\"}", Some("password")),
        ] {
            let cloud = FakeCloud::default();
            cloud
                .secrets
                .lock()
                .unwrap()
                .insert(GCP.into(), value.to_vec());
            let mut entry = json!({"env": "A", "provider": "gcp", "secret_id": GCP});
            if let Some(k) = key {
                entry["json_key"] = json!(k);
            }
            let secrets = manifest(json!([entry])).unwrap();
            assert!(
                materialize(&cloud, &secrets, &dir, &dir, 0).await.is_err(),
                "{value:?}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn put_stores_a_trimmed_line_or_raw_file() {
        let cloud = FakeCloud::default();
        let secrets = manifest(json!([
            {"env": "API_DB_PASSWORD", "provider": "oci", "secret_id": OCI},
            {"file": "keyring.json", "provider": "aws", "secret_id": AWS},
            {"env": "DB", "provider": "gcp", "secret_id": GCP, "json_key": "password"},
        ]))
        .unwrap();
        put(&cloud, &secrets, "API_DB_PASSWORD", b"fresh-password\n")
            .await
            .unwrap();
        assert_eq!(cloud.secrets.lock().unwrap()[OCI], b"fresh-password");
        put(&cloud, &secrets, "keyring.json", b"{\"k\":1}\n")
            .await
            .unwrap();
        assert_eq!(cloud.secrets.lock().unwrap()[AWS], b"{\"k\":1}\n");
        assert!(put(&cloud, &secrets, "DB", b"x").await.is_err());
        assert!(put(&cloud, &secrets, "MISSING", b"x").await.is_err());
        assert!(
            put(&cloud, &secrets, "API_DB_PASSWORD", b"a'b")
                .await
                .is_err()
        );
    }
}
