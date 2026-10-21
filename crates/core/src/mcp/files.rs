//! Audio and cover art from an assistant (migration 0143). The bytes take
//! the Studio's path: a quarantine object, then upload completion (content
//! sniff, antivirus, lossless normalization) before the asset is registered.
//!
//! - `start_upload` returns a signed PUT URL for an agent that can send
//!   files itself; `complete_upload` then registers the file.
//! - `import_file_from_url` fetches a public HTTPS link (cloud drive, file
//!   host) on the server and registers it, for chat assistants that cannot
//!   send bytes.
//! - Completion can outlast a tool call for long masters: the work goes on
//!   in the background, and `get_upload_status` reports how it ended.
//!
//! The registered asset is attached with `upsert_track` (audio) or
//! `update_release_draft` (`artwork_asset_id`).
use super::{
    oauth::Principal,
    protocol::error_code,
    tools::{ToolError, tool_error},
};
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    uploads::{self, CompleteInput, UploadInput},
};
use serde_json::{Value, json};
use sqlx::Row;
use std::time::Duration;
use tokio::{io::AsyncWriteExt, sync::Semaphore};
use uuid::Uuid;

/// How long a tool call waits for completion before it answers PROCESSING.
const COMPLETE_WAIT: Duration = Duration::from_secs(45);
/// Redirects an import follows (file hosts send one or two).
const IMPORT_MAX_HOPS: usize = 5;
/// Longest an import may take, download and completion together.
const IMPORT_DEADLINE: Duration = Duration::from_secs(15 * 60);
/// Background work older than this without an outcome was cut off (the
/// server restarted mid-way).
const STALE_MINUTES: i32 = 30;
/// Imports running at once on this server (disk and bandwidth).
static IMPORTS: Semaphore = Semaphore::const_new(4);

const NEXT_STEP: &str = "Attach it: audio with upsert_track (asset_id), cover art with update_release_draft (artwork_asset_id).";

fn kind_arg(args: &Value) -> std::result::Result<&'static str, ToolError> {
    match args.get("kind").and_then(Value::as_str) {
        Some("AUDIO") => Ok("AUDIO"),
        Some("IMAGE") => Ok("IMAGE"),
        Some("DOCUMENT") => Ok("DOCUMENT"),
        _ => Err(tool_error(
            "INVALID_ARGUMENTS",
            "kind must be AUDIO, IMAGE or DOCUMENT.",
        )),
    }
}

fn uuid_arg(args: &Value, key: &str) -> std::result::Result<Uuid, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| tool_error("INVALID_ARGUMENTS", &format!("{key} must be a UUID.")))
}

/// `start_upload`: a signed PUT for one file.
pub(super) async fn start(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let input = UploadInput {
        kind: kind_arg(args)?.to_string(),
        size_bytes: args.get("size_bytes").and_then(Value::as_i64).unwrap_or(0),
        content_type: args
            .get("content_type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        resumable: false,
    };
    let v = uploads::issue(s, &p.actor, p.org, input).await?;
    Ok(json!({
        "view": "upload",
        "upload_session_id": v["upload_session_id"],
        "asset_id": v["asset_id"],
        "expected_key": v["expected_key"],
        "upload": v["grant"],
        "next": "PUT the file's bytes to upload.url with exactly upload.headers before upload.expires_at, then call upload_file (action complete) with upload_session_id, asset_id and expected_key.",
    }))
}

/// `complete_upload`: registers a file sent to a `start_upload` URL.
pub(super) async fn complete(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let session = uuid_arg(args, "upload_session_id")?;
    let input = CompleteInput {
        asset_id: uuid_arg(args, "asset_id")?,
        expected_key: args
            .get("expected_key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    };
    // A file already registered answers at once (completion is idempotent).
    let st = uploads::status(s, &p.actor, p.org, session).await?;
    if st["status"] == "COMPLETED" {
        return Ok(ready(st["asset_id"].clone(), session));
    }
    track(s, p, session, "PUT", None).await?;
    let (state, actor, org) = (s.clone(), p.actor.clone(), p.org);
    let work = tokio::spawn(async move {
        let r = uploads::complete(&state, &actor, org, session, input).await;
        finish(&state, session, r).await
    });
    wait(work, session).await
}

/// `import_file_from_url`: fetches a public HTTPS file and registers it.
pub(super) async fn import(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let kind = kind_arg(args)?;
    let raw = args.get("url").and_then(Value::as_str).unwrap_or("");
    let url = public_url(raw)?;
    auth::rate(&s.pool, &format!("mcp-import:{}", p.grant), 30).await?;
    let permit = IMPORTS.try_acquire().map_err(|_| {
        tool_error(
            "IMPORT_BUSY",
            "Other files are being imported right now. Try again in a minute.",
        )
    })?;
    let res = fetch(url).await?;
    let host = res.url().host_str().unwrap_or("").to_string();
    let size = res.content_length().ok_or_else(|| {
        tool_error(
            "IMPORT_SIZE_UNKNOWN",
            "The link did not say how large the file is. Use a direct download link, or send the file with upload_file (action start).",
        )
    })?;
    let content_type = content_type(kind, args, &res).ok_or_else(|| {
        tool_error(
            "UPLOAD_TYPE_UNSUPPORTED",
            "Could not tell the file type from the link. Pass content_type (audio/wav, audio/flac, image/jpeg, image/png and so on).",
        )
    })?;
    let input = UploadInput {
        kind: kind.to_string(),
        size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
        content_type: content_type.clone(),
        resumable: false,
    };
    let v = uploads::issue(s, &p.actor, p.org, input).await?;
    let session: Uuid = v["upload_session_id"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Internal)?;
    let asset: Uuid = v["asset_id"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Internal)?;
    let key = v["expected_key"].as_str().unwrap_or("").to_string();
    let nonce: Uuid =
        sqlx::query_scalar("SELECT nonce FROM catalog.upload_sessions WHERE id=$1 AND org_id=$2")
            .bind(session)
            .bind(p.org)
            .fetch_one(&s.pool)
            .await
            .map_err(Error::from)?;
    track(s, p, session, "URL", Some(&host)).await?;
    let (state, actor, org) = (s.clone(), p.actor.clone(), p.org);
    let work = tokio::spawn(async move {
        let _permit = permit;
        let job = async {
            let path = std::env::temp_dir().join(format!("audeniq-import-{session}"));
            let stored = download(res, &path, size).await;
            let put = match stored {
                Ok(()) => {
                    state
                        .storage
                        .put_file(&key, &path, &content_type, &nonce.to_string())
                        .await
                }
                Err(e) => Err(e),
            };
            let _ = tokio::fs::remove_file(&path).await;
            put?;
            let input = CompleteInput {
                asset_id: asset,
                expected_key: key.clone(),
            };
            uploads::complete(&state, &actor, org, session, input).await
        };
        let r = tokio::time::timeout(IMPORT_DEADLINE, job)
            .await
            .unwrap_or(Err(Error::PolicyGate("IMPORT_TIMEOUT")));
        finish(&state, session, r).await
    });
    let mut out = wait(work, session).await?;
    out["source_host"] = json!(host);
    Ok(out)
}

/// `get_upload_status`: how a file's upload or import is going.
pub(super) async fn status(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let session = uuid_arg(args, "upload_session_id")?;
    let st = uploads::status(s, &p.actor, p.org, session).await?;
    if st["status"] == "COMPLETED" {
        return Ok(ready(st["asset_id"].clone(), session));
    }
    let job = sqlx::query(
        "SELECT status, error_code, (created_at < now() - make_interval(mins => $3)) AS stale
           FROM identity.mcp_uploads WHERE session_id=$1 AND org_id=$2",
    )
    .bind(session)
    .bind(p.org)
    .bind(STALE_MINUTES)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?;
    let (state, error) = match &job {
        Some(r) if r.get::<String, _>("status") == "FAILED" => {
            ("FAILED", r.get::<Option<String>, _>("error_code"))
        }
        Some(r) if r.get::<String, _>("status") == "PROCESSING" && r.get::<bool, _>("stale") => {
            ("FAILED", Some("IMPORT_INTERRUPTED".to_string()))
        }
        Some(r) if r.get::<String, _>("status") == "PROCESSING" => ("PROCESSING", None),
        _ if st["status"] != "ISSUED" || st["expired"] == true => ("EXPIRED", None),
        _ => ("WAITING_FOR_FILE", None),
    };
    let mut v = json!({
        "view": "upload_status",
        "upload_session_id": session,
        "asset_id": st["asset_id"],
        "state": state,
    });
    if let Some(code) = error {
        v["error_code"] = json!(code);
        v["note"] = json!(
            "The file was not registered. Fix what the error code says and start again with a new upload or import."
        );
    }
    Ok(v)
}

fn ready(asset: Value, session: Uuid) -> Value {
    json!({
        "view": "upload_status",
        "upload_session_id": session,
        "asset_id": asset,
        "state": "READY",
        "next": NEXT_STEP,
    })
}

/// Records background work for [`status`].
async fn track(
    s: &AppState,
    p: &Principal,
    session: Uuid,
    source: &str,
    host: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO identity.mcp_uploads(session_id,grant_id,org_id,source,source_host)
         VALUES($1,$2,$3,$4,$5)
         ON CONFLICT (session_id) DO UPDATE
            SET status='PROCESSING', error_code=NULL, created_at=now(), finished_at=NULL",
    )
    .bind(session)
    .bind(p.grant)
    .bind(p.org)
    .bind(source)
    .bind(host)
    .execute(&s.pool)
    .await?;
    Ok(())
}

/// Stores the outcome of background work and passes it on.
async fn finish(s: &AppState, session: Uuid, r: Result<Value>) -> Result<Value> {
    let error = r.as_ref().err().map(error_code);
    if let Err(e) = sqlx::query(
        "UPDATE identity.mcp_uploads SET status=$2, error_code=$3, finished_at=now() WHERE session_id=$1",
    )
    .bind(session)
    .bind(if error.is_some() { "FAILED" } else { "DONE" })
    .bind(error)
    .execute(&s.pool)
    .await
    {
        tracing::warn!(error = %e, %session, "mcp upload outcome not recorded");
    }
    r
}

/// The work's result if it ends within [`COMPLETE_WAIT`], else PROCESSING.
async fn wait(
    work: tokio::task::JoinHandle<Result<Value>>,
    session: Uuid,
) -> std::result::Result<Value, ToolError> {
    match tokio::time::timeout(COMPLETE_WAIT, work).await {
        Ok(Ok(Ok(v))) => Ok(ready(v["asset_id"].clone(), session)),
        Ok(Ok(Err(e))) => Err(e.into()),
        Ok(Err(_)) => Err(Error::Internal.into()),
        Err(_) => Ok(json!({
            "view": "upload_status",
            "upload_session_id": session,
            "state": "PROCESSING",
            "next": "The file is still being checked. Call get_file_status (view upload) with upload_session_id in a minute; do not upload it again.",
        })),
    }
}

/// An HTTPS URL on a named host. IP literals are refused: only names go
/// through the public-address resolver.
fn public_url(raw: &str) -> std::result::Result<url::Url, ToolError> {
    let bad = || {
        tool_error(
            "IMPORT_URL_INVALID",
            "Use a public https:// link to the file (no IP addresses, user names or passwords).",
        )
    };
    let url = crate::external_http::validate_url(raw, false).map_err(|_| bad())?;
    match url.host() {
        Some(url::Host::Domain(d)) if d.contains('.') && raw.len() <= 2048 => Ok(url),
        _ => Err(bad()),
    }
}

/// GET with redirects checked hop by hop.
async fn fetch(mut url: url::Url) -> std::result::Result<reqwest::Response, ToolError> {
    let client = crate::external_http::public_client_builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| Error::Internal)?;
    let failed = |what: &str| {
        tool_error(
            "IMPORT_FETCH_FAILED",
            &format!(
                "Could not download the file ({what}). Check that the link is public and points straight at the file."
            ),
        )
    };
    for _ in 0..=IMPORT_MAX_HOPS {
        let res = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| failed("no response"))?;
        if res.status().is_redirection() {
            let next = res
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|l| l.to_str().ok())
                .and_then(|l| url.join(l).ok())
                .ok_or_else(|| failed("bad redirect"))?;
            url = public_url(next.as_str())?;
            continue;
        }
        if !res.status().is_success() {
            return Err(failed(&format!("status {}", res.status().as_u16())));
        }
        return Ok(res);
    }
    Err(failed("too many redirects"))
}

/// The declared type: the argument, else the response header, else the
/// file name's extension. Completion sniffs the bytes either way.
fn content_type(kind: &str, args: &Value, res: &reqwest::Response) -> Option<String> {
    let given = args.get("content_type").and_then(Value::as_str);
    let header = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
    let ext = res
        .url()
        .path()
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase());
    let by_ext = match ext.as_deref() {
        Some("wav") => Some("audio/wav"),
        Some("flac") => Some("audio/flac"),
        Some("m4a") => Some("audio/mp4"),
        Some("aif" | "aiff") => Some("audio/aiff"),
        Some("wv") => Some("audio/wavpack"),
        Some("tta") => Some("audio/tta"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("pdf") => Some("application/pdf"),
        _ => None,
    };
    [given.map(str::to_owned), header, by_ext.map(str::to_owned)]
        .into_iter()
        .flatten()
        .find(|t| uploads::expected_container(kind, t).is_some())
}

/// Streams the body to `path`; exactly `size` bytes must arrive.
async fn download(mut res: reqwest::Response, path: &std::path::Path, size: u64) -> Result<()> {
    let mut f = tokio::fs::File::create(path)
        .await
        .map_err(|_| Error::Internal)?;
    let mut got: u64 = 0;
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|_| Error::PolicyGate("IMPORT_FETCH_FAILED"))?
    {
        got += chunk.len() as u64;
        if got > size {
            return Err(Error::PolicyGate("IMPORT_SIZE_MISMATCH"));
        }
        f.write_all(&chunk).await.map_err(|_| Error::Internal)?;
    }
    if got != size {
        return Err(Error::PolicyGate("IMPORT_SIZE_MISMATCH"));
    }
    f.flush().await.map_err(|_| Error::Internal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_only_public_https_names() {
        for ok in [
            "https://drive.example.com/file.wav",
            "https://files.example.org/a/b?download=1",
        ] {
            assert!(public_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://drive.example.com/file.wav",
            "https://127.0.0.1/file.wav",
            "https://[::1]/file.wav",
            "https://localhost/file.wav",
            "https://user:pw@example.com/file.wav",
            "ftp://example.com/file.wav",
            "file:///etc/passwd",
        ] {
            assert!(public_url(bad).is_err(), "{bad}");
        }
    }
}
