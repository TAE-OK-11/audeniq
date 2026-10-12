//! Host secret tool: KMS-wrapped payout keyrings and secret-manager values
//! for AWS, GCP and OCI, materialized into tmpfs. See
//! docs/BACKEND_SECURITY_2026-10-03.md (KMS) and docs/SECRET_MANAGER.md.
//!
//! No secret value, credential or cloud response body is ever printed.
use audeniq_core::cloud::{CloudError, Result};
use audeniq_core::host_secrets::{
    Clouds, Provider, keyring, private_read, secure_directory, store,
};
use std::path::PathBuf;
use std::sync::Arc;

const USAGE: &str = "usage:
  audeniq-secrets kms wrap-legacy --bundle PATH --provider aws|gcp|oci --key-id ID --legacy-key-file PATH
  audeniq-secrets kms generate    --bundle PATH --provider aws|gcp|oci --key-id ID
  audeniq-secrets kms rewrap      --bundle PATH --provider aws|gcp|oci --key-id ID
  audeniq-secrets kms materialize --bundle PATH
  audeniq-secrets secrets check       --manifest PATH
  audeniq-secrets secrets materialize --manifest PATH
  audeniq-secrets secrets put         --manifest PATH --target NAME --value-file PATH
options: --runtime-dir DIR (default /dev/shm/audeniq-secrets, mounted into the API)
         --env-dir DIR (default /dev/shm/audeniq-env, host only)
         --gid N (API group, default 10001)  --kms-concurrency 1..4 (default 2)

key ids: AWS key ARN; GCP projects/P/locations/L/keyRings/R/cryptoKeys/K;
         OCI <vault>-crypto.kms.<region>.oraclecloud.com/<key OCID>";

struct Args(Vec<String>);

impl Args {
    fn take(&mut self, key: &str) -> Result<Option<String>> {
        let Some(i) = self.0.iter().position(|a| a == key) else {
            return Ok(None);
        };
        if i + 1 >= self.0.len() {
            return Err(CloudError(format!("{key} needs a value")));
        }
        let v = self.0.remove(i + 1);
        self.0.remove(i);
        Ok(Some(v))
    }
    fn need(&mut self, key: &str) -> Result<String> {
        self.take(key)?
            .ok_or_else(|| CloudError(format!("{key} is required")))
    }
    fn path(&mut self, key: &str, default: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(
            self.take(key)?.unwrap_or_else(|| default.into()),
        ))
    }
    fn number(&mut self, key: &str, default: u32) -> Result<u32> {
        match self.take(key)? {
            Some(v) => v
                .parse()
                .map_err(|_| CloudError(format!("{key} must be a number"))),
            None => Ok(default),
        }
    }
    fn done(self) -> Result<()> {
        match self.0.first() {
            Some(extra) => Err(CloudError(format!("unexpected argument {extra}"))),
            None => Ok(()),
        }
    }
}

async fn run(mut args: Args) -> Result<String> {
    if args.0.len() < 2 {
        return Err(CloudError(USAGE.into()));
    }
    let group = args.0.remove(0);
    let command = args.0.remove(0);
    let runtime = args.path("--runtime-dir", "/dev/shm/audeniq-secrets")?;
    let gid = args.number("--gid", 10001)?;
    match group.as_str() {
        "kms" => kms(&command, args, runtime, gid).await,
        "secrets" => secrets(&command, args, runtime, gid).await,
        _ => Err(CloudError(USAGE.into())),
    }
}

fn read_bundle(path: &std::path::Path) -> Result<keyring::Bundle> {
    if !path.exists() {
        return Ok(keyring::Bundle::default());
    }
    keyring::Bundle::parse(&private_read(path, 65536)?)
}

async fn kms(command: &str, mut args: Args, runtime: PathBuf, gid: u32) -> Result<String> {
    let bundle_path = PathBuf::from(args.need("--bundle")?);
    let concurrency = args.number("--kms-concurrency", 2)? as usize;
    let provider = args.take("--provider")?;
    let key_id = args.take("--key-id")?;
    let legacy = args.take("--legacy-key-file")?;
    args.done()?;
    let scratch = secure_directory(&runtime, Some(gid))?;
    let bundle = read_bundle(&bundle_path)?;
    let cloud = Arc::new(Clouds::default());
    if command == "materialize" {
        if provider.is_some() || key_id.is_some() {
            return Err(CloudError(
                "materialize reads providers from the bundle; use rewrap to switch".into(),
            ));
        }
        keyring::materialize(
            cloud,
            &bundle,
            &scratch.join("payout-keyring.json"),
            gid,
            concurrency,
        )
        .await?;
        return Ok(format!(
            "KMS keyring materialized; active version {}",
            bundle.active_version
        ));
    }
    let provider =
        Provider::parse(&provider.ok_or_else(|| CloudError("--provider is required".into()))?)?;
    let key_id = key_id.ok_or_else(|| CloudError("--key-id is required".into()))?;
    let bundle = match command {
        "wrap-legacy" => {
            let file = legacy.ok_or_else(|| CloudError("--legacy-key-file is required".into()))?;
            let key = private_read(std::path::Path::new(&file), 256)?;
            keyring::wrap_legacy(cloud.as_ref(), bundle, provider, &key_id, &key).await?
        }
        "generate" => keyring::generate(cloud.as_ref(), bundle, provider, &key_id).await?,
        "rewrap" => keyring::rewrap(cloud, &bundle, provider, &key_id, concurrency).await?,
        _ => return Err(CloudError(USAGE.into())),
    };
    audeniq_core::host_secrets::atomic_write(&bundle_path, &bundle.to_json()?, None)?;
    Ok(format!(
        "KMS keyring {command} completed; active version {}",
        bundle.active_version
    ))
}

async fn secrets(command: &str, mut args: Args, runtime: PathBuf, gid: u32) -> Result<String> {
    let manifest = store::parse_manifest(&private_read(
        std::path::Path::new(&args.need("--manifest")?),
        65536,
    )?)?;
    let env_dir = args.path("--env-dir", "/dev/shm/audeniq-env")?;
    let target = args.take("--target")?;
    let value_file = args.take("--value-file")?;
    args.done()?;
    let cloud = Clouds::default();
    match command {
        "check" => Ok(format!("secret manifest valid; {} secrets", manifest.len())),
        "materialize" => {
            let keys = secure_directory(&runtime, Some(gid))?;
            let env = secure_directory(&env_dir, None)?;
            if env.canonicalize().ok() == keys.canonicalize().ok() {
                return Err(CloudError(
                    "secrets.env must not share the directory mounted into the API".into(),
                ));
            }
            let (envs, files) = store::materialize(&cloud, &manifest, &keys, &env, gid).await?;
            Ok(format!(
                "secrets materialized; {envs} environment values, {files} files"
            ))
        }
        "put" => {
            let target = target.ok_or_else(|| CloudError("--target is required".into()))?;
            let file = value_file.ok_or_else(|| CloudError("--value-file is required".into()))?;
            let value = private_read(std::path::Path::new(&file), store::MAX_VALUE)?;
            store::put(&cloud, &manifest, &target, &value).await?;
            Ok(format!(
                "secret {target} stored as a new version; run materialize to use it"
            ))
        }
        _ => Err(CloudError(USAGE.into())),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Everything this tool creates is private by default.
    unsafe { libc::umask(0o077) };
    let args = Args(std::env::args().skip(1).collect());
    match run(args).await {
        Ok(message) => println!("{message}"),
        Err(error) => {
            // Messages are fixed operation texts: no value, key or response body.
            eprintln!("audeniq-secrets: {error}");
            std::process::exit(1);
        }
    }
}
