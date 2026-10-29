//! The `audeniq-secrets` host binary and deploy.sh's use of the secrets it
//! materializes: argument and input checks happen before any cloud call,
//! failures print no value, and deploy.sh passes `SECRETS_ENV_FILE` to
//! compose after production.env only when it is a private tmpfs file.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn shm_dir() -> PathBuf {
    let dir = PathBuf::from("/dev/shm").join(format!(
        "audeniq-cli-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn write(path: &Path, text: &str, mode: u32) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn tool(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_audeniq-secrets"))
        .args(args)
        // No cloud credentials anywhere: any network attempt fails closed.
        .env("HOME", dir)
        .env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("GOOGLE_APPLICATION_CREDENTIALS")
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn manifest_and_bundle_inputs_are_checked_before_cloud_calls() {
    let dir = shm_dir();
    let manifest = dir.join("manifest.json");
    write(
        &manifest,
        r#"{"version":1,"secrets":[{"env":"API_DB_PASSWORD","provider":"oci","secret_id":"ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample.secret0123"}]}"#,
        0o600,
    );
    let m = manifest.to_str().unwrap();
    let (ok, out) = tool(&dir, &["secrets", "check", "--manifest", m]);
    assert!(ok, "{out}");
    assert!(out.contains("1 secrets"));

    std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!tool(&dir, &["secrets", "check", "--manifest", m]).0);
    write(
        &manifest,
        r#"{"version":1,"secrets":[{"env":"AUDENIQ_IMAGE","provider":"aws","secret_id":"x"}]}"#,
        0o600,
    );
    assert!(!tool(&dir, &["secrets", "check", "--manifest", m]).0);

    // put: the value file must be private; the value never appears in output.
    write(
        &manifest,
        r#"{"version":1,"secrets":[{"env":"A","provider":"oci","secret_id":"ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample.secret0123"}]}"#,
        0o600,
    );
    let value = dir.join("value");
    write(&value, "very-secret-value-123", 0o644);
    let (ok, out) = tool(
        &dir,
        &[
            "secrets",
            "put",
            "--manifest",
            m,
            "--target",
            "A",
            "--value-file",
            value.to_str().unwrap(),
        ],
    );
    assert!(!ok);
    assert!(!out.contains("very-secret-value-123"), "{out}");
    // Private value, but no OCI config: fails closed, still silent about the value.
    std::fs::set_permissions(&value, std::fs::Permissions::from_mode(0o600)).unwrap();
    let (ok, out) = tool(
        &dir,
        &[
            "secrets",
            "put",
            "--manifest",
            m,
            "--target",
            "A",
            "--value-file",
            value.to_str().unwrap(),
        ],
    );
    assert!(!ok);
    assert!(!out.contains("very-secret-value-123"), "{out}");

    // KMS: invalid key ids and unknown providers refuse; materialize needs a bundle.
    let runtime = dir.join("run");
    let r = runtime.to_str().unwrap();
    let bundle = dir.join("wrapped.json");
    let b = bundle.to_str().unwrap();
    let gid = unsafe { libc::getegid() }.to_string();
    for args in [
        vec![
            "kms",
            "generate",
            "--bundle",
            b,
            "--provider",
            "oci",
            "--key-id",
            "ocid1.key.oc1.x.y.z",
        ],
        vec![
            "kms",
            "generate",
            "--bundle",
            b,
            "--provider",
            "azure",
            "--key-id",
            "x",
        ],
        vec![
            "kms",
            "generate",
            "--bundle",
            b,
            "--provider",
            "aws",
            "--key-id",
            "alias/payout",
        ],
        vec!["kms", "materialize", "--bundle", b],
        vec!["kms", "materialize", "--bundle", b, "--provider", "aws"],
        vec!["kms", "nonsense", "--bundle", b],
        vec!["kms"],
    ] {
        let mut args = args;
        args.extend(["--runtime-dir", r, "--gid", &gid]);
        let (ok, out) = tool(&dir, &args);
        assert!(!ok, "{args:?}: {out}");
    }
    assert!(!bundle.exists());
    // Runtime directories must be tmpfs.
    let disk = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("not-tmpfs-{}", uuid::Uuid::new_v4()));
    let (ok, out) = tool(
        &dir,
        &[
            "secrets",
            "materialize",
            "--manifest",
            m,
            "--runtime-dir",
            disk.to_str().unwrap(),
            "--gid",
            &gid,
        ],
    );
    let _ = std::fs::remove_dir_all(&disk);
    assert!(!ok, "{out}");
    std::fs::remove_dir_all(dir).unwrap();
}

struct Deploy {
    _task: tempdir::Dir,
    status: std::process::ExitStatus,
    stderr: String,
    compose_calls: Vec<Vec<String>>,
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Run deploy.sh --status with a docker stub that logs its arguments.
fn deploy_status(secrets_line: &str) -> Deploy {
    deploy_status_with_env_mode(secrets_line, 0o600)
}

fn deploy_status_with_env_mode(secrets_line: &str, mode: u32) -> Deploy {
    let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
    let task = std::env::temp_dir().join(format!("audeniq-deploy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(task.join("pgbouncer")).unwrap();
    std::fs::copy(deploy.join("deploy.sh"), task.join("deploy.sh")).unwrap();
    for f in [
        "pgbouncer/Dockerfile",
        "compose.production.yaml",
        "check-cpu.sh",
    ] {
        std::fs::write(task.join(f), "").unwrap();
    }
    write(
        &task.join("production.env"),
        &format!("AUDENIQ_IMAGE=ghcr.io/test/a@sha256:x\n{secrets_line}"),
        mode,
    );
    let log = task.join("log");
    write(
        &task.join("docker"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n[ \"$1 $2\" = 'volume inspect' ] && exit 1\nexit 0\n",
            log.display()
        ),
        0o755,
    );
    let output = Command::new("bash")
        .arg(task.join("deploy.sh"))
        .arg("--status")
        .env(
            "PATH",
            format!("{}:{}", task.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();
    let compose_calls = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("compose "))
        .map(|l| l.split(' ').map(str::to_string).collect())
        .collect();
    Deploy {
        _task: tempdir::Dir(task),
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        compose_calls,
    }
}

fn env_files(call: &[String]) -> Vec<String> {
    call.windows(2)
        .filter(|w| w[0] == "--env-file")
        .map(|w| w[1].clone())
        .collect()
}

#[test]
fn deploy_reads_materialized_secrets_after_production_env() {
    let plain = deploy_status("");
    assert!(plain.status.success(), "{}", plain.stderr);
    assert_eq!(env_files(&plain.compose_calls[0]), vec!["production.env"]);

    let dir = shm_dir();
    let secrets = dir.join("secrets.env");
    write(&secrets, "POSTGRES_PASSWORD='x'\n", 0o600);
    let line = format!("SECRETS_ENV_FILE={}\n", secrets.display());
    let with = deploy_status(&line);
    assert!(with.status.success(), "{}", with.stderr);
    assert_eq!(
        env_files(&with.compose_calls[0]),
        vec!["production.env".to_string(), secrets.display().to_string()]
    );

    // Readable by others, overriding the image, or missing: refused before compose.
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o644)).unwrap();
    let loose = deploy_status(&line);
    assert!(!loose.status.success() && loose.compose_calls.is_empty());
    write(&secrets, "AUDENIQ_IMAGE='ghcr.io/evil'\n", 0o600);
    let image = deploy_status(&line);
    assert!(!image.status.success() && image.compose_calls.is_empty());
    std::fs::remove_file(&secrets).unwrap();
    let missing = deploy_status(&line);
    assert!(!missing.status.success() && missing.compose_calls.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn deploy_refuses_public_production_env_before_compose() {
    let loose = deploy_status_with_env_mode("", 0o644);
    assert!(!loose.status.success());
    assert!(loose.compose_calls.is_empty());
    assert!(
        loose
            .stderr
            .contains("production.env must be owned by the deploy user with mode 0600"),
        "{}",
        loose.stderr
    );
}
