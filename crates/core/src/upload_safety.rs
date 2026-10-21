//! Upload admission: inspect the exact frozen bytes before any decoder sees them.
use crate::error::{Error, Result};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const RULE_VERSION: &str = "1";

pub(crate) fn signature_bytes(value: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let encoded = value
        .strip_prefix("data:image/png;base64,")
        .filter(|v| (32..=59_978).contains(&v.len()))
        .ok_or(Error::InvalidCode("SIGNATURE_INVALID"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Error::InvalidCode("SIGNATURE_INVALID"))?;
    if bytes.len() < 45 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Error::InvalidCode("SIGNATURE_INVALID"));
    }
    Ok(bytes)
}

/// Electronic signatures are hashed as exact bytes. Preserve that evidence,
/// accepting only CRC-checked, metadata-free PNGs with a complete bounded
/// pixel stream. Neither arbitrary data URLs nor appended bytes can be saved.
pub async fn signature(state: &crate::api::AppState, value: &str) -> Result<String> {
    // These exact bytes already passed (same rule version, and the AV scan
    // when it is on): a draft re-saved (every autosave) or a signature
    // re-sent is not decoded or re-sanitized.
    let source_sha256 = crate::domain::sha256_hex(value);
    let admitted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.inline_file_safety WHERE source_sha256=$1 AND rule_version=$2 AND (NOT $3 OR antivirus_status='SCANNED'))",
    )
    .bind(&source_sha256)
    .bind(RULE_VERSION)
    .bind(state.antivirus_enabled)
    .fetch_one(&state.pool)
    .await?;
    if admitted {
        return Ok(value.to_string());
    }
    let bytes = signature_bytes(value)?;
    // A small image of its own slot pool, waited for briefly: signing a
    // contract used to fail at once whenever an audio upload was converting
    // (it shared the single transcode slot through try_acquire).
    let slot = tokio::time::timeout(
        Duration::from_secs(20),
        state.inline_slots.clone().acquire_owned(),
    )
    .await
    .map_err(|_| Error::UploadBusy)?
    .map_err(|_| Error::Internal)?;
    let workspace = Workspace::new()?;
    let source = workspace.0.join("signature.png");
    let target = workspace.0.join("validated.png");
    tokio::fs::write(&source, &bytes)
        .await
        .map_err(|_| Error::Storage)?;
    scan(&source, bytes.len() as u64, state.antivirus_enabled).await?;
    let len = bytes.len();
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let _workspace = workspace;
        sanitize(
            &source,
            &target,
            "application/x-audeniq-signature",
            &_workspace.0,
        )
        .map_err(|e| {
            // 기기·브라우저별 PNG 차이로 거절된 서명을 운영 로그에서 찾을 수 있게
            tracing::warn!(error = ?e, bytes = len, "inline signature rejected by sanitizer");
            Error::InvalidCode("SIGNATURE_INVALID")
        })
    })
    .await
    .map_err(|_| Error::Internal)??;
    sqlx::query("INSERT INTO catalog.inline_file_safety(source_sha256,rule_version,antivirus_status) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(&source_sha256).bind(RULE_VERSION)
        .bind(antivirus_status(state.antivirus_enabled)).execute(&state.pool).await?;
    Ok(value.to_string())
}

fn visit_inline<'a>(value: &'a serde_json::Value, key: &str, found: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s)
            if !s.is_empty()
                && (key == "signature" || s.starts_with("data:") || s.starts_with("blob:")) =>
        {
            found.push(s)
        }
        serde_json::Value::Array(a) => {
            for v in a {
                visit_inline(v, key, found);
            }
        }
        serde_json::Value::Object(o) => {
            for (k, v) in o {
                visit_inline(v, k, found);
            }
        }
        _ => (),
    }
}

/// Catalog JSON can contain the same image as the explicit signing endpoint.
/// Validate those images at intake too; a draft cannot bypass file admission.
pub async fn check_inline_files(
    state: &crate::api::AppState,
    value: &serde_json::Value,
) -> Result<()> {
    let mut images = Vec::new();
    visit_inline(value, "", &mut images);
    let unique: std::collections::BTreeSet<_> = images.into_iter().collect();
    if unique.len() > 8 {
        return Err(Error::InvalidCode("SIGNATURE_INVALID"));
    }
    for image in unique {
        signature(state, image).await?;
    }
    Ok(())
}

/// Reads make no parser/AV calls. One batched receipt lookup removes old,
/// uninspected embedded images from the response, preserving the DB evidence.
pub async fn visible_inline_files(
    pool: &sqlx::PgPool,
    mut value: serde_json::Value,
) -> Result<serde_json::Value> {
    let mut images = Vec::new();
    visit_inline(&value, "", &mut images);
    if images.is_empty() {
        return Ok(value);
    }
    // Each embedded file is hashed once (a release list carries up to 100
    // covers of ~48 KB); the rewrite below matches the strings themselves.
    images.sort_unstable();
    images.dedup();
    let hashed: Vec<(&str, String)> = images
        .into_iter()
        .map(|s| (s, crate::domain::sha256_hex(s)))
        .collect();
    let hashes: Vec<String> = hashed
        .iter()
        .map(|(_, h)| h.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(512)
        .collect();
    let admitted: Vec<String> = sqlx::query_scalar("SELECT source_sha256 FROM catalog.inline_file_safety WHERE source_sha256=ANY($1) AND rule_version=$2 AND (NOT $3 OR antivirus_status='SCANNED')")
        .bind(hashes).bind(RULE_VERSION).bind(crate::config::antivirus_enabled()?).fetch_all(pool).await?;
    let admitted: std::collections::BTreeSet<_> = admitted.into_iter().collect();
    let hidden: std::collections::HashSet<String> = hashed
        .into_iter()
        .filter(|(_, h)| !admitted.contains(h))
        .map(|(s, _)| s.to_owned())
        .collect();
    if hidden.is_empty() {
        return Ok(value);
    }
    fn hide(value: &mut serde_json::Value, key: &str, hidden: &std::collections::HashSet<String>) {
        match value {
            serde_json::Value::String(s)
                if !s.is_empty()
                    && (key == "signature" || s.starts_with("data:") || s.starts_with("blob:")) =>
            {
                if hidden.contains(s.as_str()) {
                    s.clear();
                }
            }
            serde_json::Value::Array(a) => {
                for v in a {
                    hide(v, key, hidden);
                }
            }
            serde_json::Value::Object(o) => {
                for (k, v) in o {
                    hide(v, k, hidden);
                }
            }
            _ => (),
        }
    }
    hide(&mut value, "", &hidden);
    Ok(value)
}

pub struct Workspace(pub PathBuf);
impl Workspace {
    pub fn new() -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!("audeniq-inspect-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| Error::Storage)?;
        Ok(Self(path))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn socket() -> PathBuf {
    std::env::var_os("UPLOAD_SCANNER_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/run/audeniq-av/clamd.sock".into())
}

/// Check the daemon's loaded database date, not a file mtime that may have
/// changed before the engine reloaded. No bypass when unavailable or stale.
async fn fresh(socket: &Path) -> Result<()> {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|_| Error::UploadBusy)?;
    stream
        .write_all(b"zVERSION\0")
        .await
        .map_err(|_| Error::UploadBusy)?;
    let reply = reply(&mut stream).await?;
    let date = reply.rsplit('/').next().ok_or(Error::UploadBusy)?;
    let date = chrono::NaiveDateTime::parse_from_str(date, "%a %b %e %H:%M:%S %Y")
        .map_err(|_| Error::UploadBusy)?;
    let age = chrono::Utc::now().naive_utc() - date;
    if age.num_seconds() < -3600 || age > chrono::Duration::hours(72) {
        return Err(Error::UploadBusy);
    }
    Ok(())
}

async fn reply(stream: &mut tokio::net::UnixStream) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        let byte = stream.read_u8().await.map_err(|_| Error::UploadBusy)?;
        if byte == 0 {
            break;
        }
        if bytes.len() >= 1024 {
            return Err(Error::UploadBusy);
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).map_err(|_| Error::UploadBusy)
}

pub fn antivirus_status(enabled: bool) -> &'static str {
    if enabled { "SCANNED" } else { "SKIPPED" }
}
pub fn safety_status(antivirus_status: &str) -> &'static str {
    if antivirus_status == "SCANNED" {
        "VERIFIED"
    } else {
        "SANITIZED"
    }
}
pub async fn scan(path: &Path, expected_size: u64, enabled: bool) -> Result<()> {
    if enabled {
        scan_on(path, expected_size, &socket()).await
    } else {
        Ok(())
    }
}

async fn scan_on(path: &Path, expected_size: u64, socket: &Path) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(120), async {
        fresh(socket).await?;
        let mut stream = tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|_| Error::UploadBusy)?;
        stream
            .write_all(b"zINSTREAM\0")
            .await
            .map_err(|_| Error::UploadBusy)?;
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|_| Error::Storage)?;
        let mut buf = [0; 64 * 1024];
        let mut size = 0;
        loop {
            let n = file.read(&mut buf).await.map_err(|_| Error::Storage)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > expected_size || size > crate::uploads::MAX_AUDIO_BYTES as u64 {
                return Err(Error::Conflict);
            }
            stream
                .write_u32(n as u32)
                .await
                .map_err(|_| Error::UploadBusy)?;
            stream
                .write_all(&buf[..n])
                .await
                .map_err(|_| Error::UploadBusy)?;
        }
        if size != expected_size {
            return Err(Error::Conflict);
        }
        stream.write_u32(0).await.map_err(|_| Error::UploadBusy)?;
        match reply(&mut stream).await?.as_str() {
            "stream: OK" => Ok(()),
            value if value.starts_with("stream: ") && value.ends_with(" FOUND") => {
                Err(Error::PolicyGate("UPLOAD_UNSAFE_FILE"))
            }
            _ => Err(Error::UploadBusy),
        }
    })
    .await
    .map_err(|_| Error::UploadBusy)?
}

/// Rasterize PDFs / decode and re-encode images with `audeniq-music photo`.
/// Each child cannot read secrets, make network connections or write outside
/// its outputs.
pub fn sanitize(src: &Path, dst: &Path, mime: &str, workspace: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    if mime == "application/pdf" {
        // Untrusted half: parse and render the pages to raw RGB frames only.
        let frames = workspace.join("pages.frames");
        let mut rasterize = crate::photo::command("pdf-rasterize");
        rasterize.arg(src).arg(&frames);
        run_sanitizer(&mut rasterize, &[src], &frames, deadline)?;
        // The assembler validates page count, sizes, pixel budget and exact
        // frame lengths before writing an image-only PDF, so a compromised
        // renderer can contribute nothing but pixels.
        let mut assemble = crate::photo::command("pdf-assemble");
        assemble.arg(&frames).arg(dst);
        run_sanitizer(&mut assemble, &[&frames], dst, deadline)?;
    } else {
        let mut command = crate::photo::command("sanitize");
        command.arg(src).arg(dst).arg(mime);
        run_sanitizer(&mut command, &[src], dst, deadline)?;
    }
    let len = std::fs::metadata(dst).map_err(|_| Error::UploadBusy)?.len();
    if len == 0 || len > crate::uploads::MAX_DOCUMENT_BYTES as u64 {
        return Err(Error::PolicyGate("UPLOAD_SANITIZATION_FAILED"));
    }
    Ok(())
}

fn run_sanitizer(
    command: &mut std::process::Command,
    inputs: &[&Path],
    output: &Path,
    deadline: std::time::Instant,
) -> Result<()> {
    crate::parser_sandbox::restrict(command, inputs, &[output]).map_err(|_| Error::UploadBusy)?;
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    crate::local_analyzer::run_timeout(command, &[0], remaining)
        .map_err(|_| Error::PolicyGate("UPLOAD_SANITIZATION_FAILED"))?;
    Ok(())
}

/// Production refuses uninspected legacy assets too. Tests may seed historical
/// fixtures, but a real upload always uses the mandatory gate above.
pub async fn require_verified(
    pool: &sqlx::PgPool,
    org: uuid::Uuid,
    asset: uuid::Uuid,
) -> Result<()> {
    require_policy(pool, org, asset, crate::config::antivirus_enabled()?).await
}

pub async fn require_policy(
    pool: &sqlx::PgPool,
    org: uuid::Uuid,
    asset: uuid::Uuid,
    antivirus: bool,
) -> Result<()> {
    if std::env::var("APP_ENV").as_deref() != Ok("production") {
        let uploaded: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog.upload_sessions WHERE org_id=$1 AND asset_id=$2)",
        )
        .bind(org)
        .bind(asset)
        .fetch_one(pool)
        .await?;
        if !uploaded {
            return Ok(());
        }
    }
    let verified: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.asset_safety s JOIN catalog.assets a ON a.id=s.asset_id AND a.org_id=s.org_id WHERE s.org_id=$1 AND s.asset_id=$2 AND s.safe_sha256=a.sha256 AND s.safe_key=a.object_key AND s.rule_version=$3 AND (NOT $4 OR s.antivirus_status='SCANNED'))")
        .bind(org).bind(asset).bind(RULE_VERSION).bind(antivirus).fetch_one(pool).await?;
    if !verified {
        return Err(Error::PolicyGate("UPLOAD_REINSPECTION_REQUIRED"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_antivirus_performs_no_file_or_socket_io() {
        assert!(
            scan(Path::new("/nonexistent/upload"), 100, false)
                .await
                .is_ok()
        );
        assert_eq!(antivirus_status(false), "SKIPPED");
        assert_eq!(safety_status("SKIPPED"), "SANITIZED");
        assert_eq!(safety_status("SCANNED"), "VERIFIED");
    }

    async fn fake_scan(verdict: &str) -> ErrorOrSuccess {
        let workspace = Workspace::new().unwrap();
        let socket = workspace.0.join("scan.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let source = workspace.0.join("source");
        let bytes = b"the exact frozen bytes";
        tokio::fs::write(&source, bytes).await.unwrap();
        let verdict = verdict.to_string();
        let daemon = tokio::spawn(async move {
            let (mut version, _) = listener.accept().await.unwrap();
            let mut command = [0; 9];
            version.read_exact(&mut command).await.unwrap();
            assert_eq!(&command, b"zVERSION\0");
            let timestamp = chrono::Utc::now();
            let date = timestamp.format("%a %b %e %H:%M:%S %Y");
            version
                .write_all(format!("ClamAV test/1/{date}\0").as_bytes())
                .await
                .unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut command = [0; 10];
            stream.read_exact(&mut command).await.unwrap();
            assert_eq!(&command, b"zINSTREAM\0");
            let mut actual = Vec::new();
            loop {
                let n = stream.read_u32().await.unwrap();
                if n == 0 {
                    break;
                }
                assert!(n <= 65536);
                let mut chunk = vec![0; n as usize];
                stream.read_exact(&mut chunk).await.unwrap();
                actual.extend_from_slice(&chunk);
            }
            assert_eq!(actual, bytes);
            stream
                .write_all(format!("{verdict}\0").as_bytes())
                .await
                .unwrap();
        });
        let result = scan_on(&source, bytes.len() as u64, &socket).await;
        daemon.await.unwrap();
        match result {
            Ok(()) => ErrorOrSuccess::Ok,
            Err(Error::PolicyGate(code)) => ErrorOrSuccess::Rejected(code),
            Err(Error::UploadBusy) => ErrorOrSuccess::Unavailable,
            Err(_) => panic!("unexpected scan error"),
        }
    }
    #[derive(Debug, PartialEq)]
    enum ErrorOrSuccess {
        Ok,
        Rejected(&'static str),
        Unavailable,
    }

    #[tokio::test]
    async fn scanner_streams_exact_bytes_and_accepts_only_explicit_clean_verdict() {
        assert_eq!(fake_scan("stream: OK").await, ErrorOrSuccess::Ok);
        assert_eq!(
            fake_scan("stream: Eicar-Test-Signature FOUND").await,
            ErrorOrSuccess::Rejected("UPLOAD_UNSAFE_FILE")
        );
        for reply in [
            "stream: size limit exceeded ERROR",
            "stream: unknown",
            "OK",
            "stream: OK\n",
        ] {
            assert_eq!(fake_scan(reply).await, ErrorOrSuccess::Unavailable);
        }
    }

    #[tokio::test]
    async fn absent_or_stale_scanner_is_fail_closed() {
        let workspace = Workspace::new().unwrap();
        assert!(matches!(
            scan_on(
                &workspace.0.join("source"),
                1,
                &workspace.0.join("missing.sock")
            )
            .await,
            Err(Error::UploadBusy)
        ));
        for age in [chrono::Duration::days(4), chrono::Duration::days(-1)] {
            let socket = workspace.0.join(format!("{}.sock", uuid::Uuid::new_v4()));
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let task = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0; 9];
                stream.read_exact(&mut buf).await.unwrap();
                let timestamp = chrono::Utc::now() - age;
                let date = timestamp.format("%a %b %e %H:%M:%S %Y");
                stream
                    .write_all(format!("ClamAV test/1/{date}\0").as_bytes())
                    .await
                    .unwrap();
            });
            assert!(matches!(
                scan_on(&workspace.0.join("source"), 1, &socket).await,
                Err(Error::UploadBusy)
            ));
            task.await.unwrap();
        }
    }
}
