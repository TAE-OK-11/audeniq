//! Upload admission: inspect the exact frozen bytes before any decoder sees them.
use crate::error::{Error, Result};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const RULE_VERSION: &str = "1";

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

pub async fn scan(path: &Path, expected_size: u64) -> Result<()> {
    scan_on(path, expected_size, &socket()).await
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

/// Rasterize PDFs / decode and re-encode images. Child cannot read secrets,
/// make network connections or write outside this private processing directory.
pub fn sanitize(src: &Path, dst: &Path, mime: &str, workspace: &Path) -> Result<()> {
    let helper = std::env::var_os("UPLOAD_SANITIZER")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/usr/local/lib/audeniq/sanitize-upload.py".into());
    let mut command = std::process::Command::new("python3");
    command.arg(&helper).arg(src).arg(dst).arg(mime);
    crate::parser_sandbox::restrict(&mut command, &[src, &helper], &[workspace])
        .map_err(|_| Error::UploadBusy)?;
    crate::local_analyzer::run_timeout(&mut command, &[0], Duration::from_secs(120))
        .map_err(|_| Error::PolicyGate("UPLOAD_SANITIZATION_FAILED"))?;
    let len = std::fs::metadata(dst).map_err(|_| Error::UploadBusy)?.len();
    if len == 0 || len > crate::uploads::MAX_DOCUMENT_BYTES as u64 {
        return Err(Error::PolicyGate("UPLOAD_SANITIZATION_FAILED"));
    }
    Ok(())
}

/// Production refuses uninspected legacy assets too. Tests may seed historical
/// fixtures, but a real upload always uses the mandatory gate above.
pub async fn require_verified(
    pool: &sqlx::PgPool,
    org: uuid::Uuid,
    asset: uuid::Uuid,
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
    let verified: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.asset_safety s JOIN catalog.assets a ON a.id=s.asset_id AND a.org_id=s.org_id WHERE s.org_id=$1 AND s.asset_id=$2 AND s.safe_sha256=a.sha256 AND s.safe_key=a.object_key AND s.rule_version=$3)")
        .bind(org).bind(asset).bind(RULE_VERSION).fetch_one(pool).await?;
    if !verified {
        return Err(Error::PolicyGate("UPLOAD_REINSPECTION_REQUIRED"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
