//! ERN 3.8.2 XSD validation for generated `NewReleaseMessage` documents.
//!
//! Contract-free F6 groundwork: before any real partner contract exists we
//! can still prove that every interchange message we *would* send is
//! schema-valid. Validation runs inside `prepare_release` (Stage 3) and is
//! fail-closed: an invalid message never becomes a persisted
//! `distribution.ddex_messages` row.
//!
//! The schema files are vendored at `schemas/ddex/ern-382/` (see its
//! README for provenance and licence). Validation itself is delegated to
//! `xmllint` (libxml2), the same "system binary the worker shells out to"
//! pattern the QC pipeline already uses for `ffmpeg`/`ffprobe`.

use crate::error::{Error, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

/// Vendored ERN 3.8.2 schema, relative to the workspace root of the
/// `audeniq-core` crate directory layout (`crates/core/../../schemas/...`).
/// Resolved at runtime from the crate's manifest dir so tests and the worker
/// find it regardless of the process working directory.
fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/ddex/ern-382/release-notification.xsd")
}

/// Validate an ERN 3.8.2 XML document against the vendored XSD.
///
/// Returns `Ok(())` when the document is schema-valid. Returns
/// `Err(Error::PolicyGate("ERN_XSD_INVALID"))` with the first lines of the
/// validator diagnostics in the error detail when it is not, and
/// `Err(Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))` when `xmllint`
/// is missing or cannot be executed — a hard environment error, never a
/// silent skip: an unvalidated message must not be treated as valid.
pub fn validate_ern_382_xml(xml: &str) -> Result<()> {
    let schema = schema_path();
    if !schema.is_file() {
        return Err(Error::PolicyGate("ERN_XSD_SCHEMA_MISSING"));
    }
    // The document goes through a temp file, not a stdin pipe: xmllint
    // writes diagnostics to stderr, and a large invalid document could fill
    // the stderr pipe while we block on the child — a classic pipe
    // deadlock. A file argument avoids all three pipes entirely.
    let doc = TempDoc::create(xml.as_bytes())?;
    run_xmllint(&schema, &doc.path)
}

/// A temp XML document that always cleans itself up, even if validation
/// panics or returns early. The name is pid + UUID v4 and the file is
/// created with `create_new`, so a concurrent validator (or a hostile
/// /tmp neighbor) can neither collide with nor pre-create it — the old
/// pid+nanoseconds name was guessable and racy.
struct TempDoc {
    path: PathBuf,
}

impl TempDoc {
    /// An empty temp file. The name is pid + UUID v4 and the file is
    /// created with `create_new`, so a concurrent validator (or a hostile
    /// /tmp neighbor) can neither collide with nor pre-create it — the old
    /// pid+nanoseconds name was guessable and racy.
    fn empty() -> Result<Self> {
        for _ in 0..8 {
            let path = std::env::temp_dir().join(format!(
                "audeniq-ern-{}-{}.xml",
                std::process::id(),
                Uuid::new_v4().as_simple()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE")),
            }
        }
        Err(Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))
    }

    fn create(xml: &[u8]) -> Result<Self> {
        let this = Self::empty()?;
        // On write failure `this` drops here and the Drop impl removes the
        // partial file — no leak, unlike the old create-then-write order.
        std::fs::write(&this.path, xml)
            .map_err(|_| Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
        Ok(this)
    }
}

impl Drop for TempDoc {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn run_xmllint(schema: &std::path::Path, doc: &std::path::Path) -> Result<()> {
    // xmllint's diagnostics go to a temp *file*, not a pipe: with a pipe,
    // a pathological document's error spew (>64KB) would block the child
    // on write while we block on its exit — a classic pipe deadlock that
    // only the 60s timeout would break. Files have no such rendezvous.
    // stdout is empty under --noout, so it goes to null.
    let err_log = TempDoc::empty()?;
    let err_sink = std::fs::File::create(&err_log.path)
        .map_err(|_| Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
    let mut child = Command::new("xmllint")
        // --nonet: never fetch external DTDs/schemas/entities over the
        // network (ddex-suite's parser hardens the same way with
        // allow_network=false). We only validate our own generated
        // documents, but one flag removes the whole class.
        .args(["--noout", "--nonet", "--schema"])
        .arg(schema)
        .arg(doc)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(err_sink))
        .spawn()
        .map_err(|_| Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
    // xmllint on a local document returns quickly; guard against a hung
    // validator the same way the QC pipeline guards ffmpeg.
    let status = wait_status_with_timeout(&mut child, Duration::from_secs(60))
        .ok_or(Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
    if status.success() {
        return Ok(());
    }
    // Bound the diagnostics: a pathological document could make the log
    // large; we only surface the first 8 lines / 500 chars.
    let detail: String = std::fs::File::open(&err_log.path)
        .ok()
        .and_then(|f| {
            use std::io::Read;
            let mut buf = Vec::with_capacity(4096);
            // 16KB is far more than the 8 lines we keep.
            f.take(16 * 1024).read_to_end(&mut buf).ok()?;
            let text = String::from_utf8_lossy(&buf);
            Some(
                text.lines()
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(" | ")
                    .chars()
                    .take(500)
                    .collect(),
            )
        })
        .unwrap_or_default();
    // The gate code stays a plain PolicyGate so the API error surface is
    // unchanged; the validator diagnostics go to the log, never the client.
    tracing::warn!(gate = "ERN_XSD_INVALID", detail = %detail, "ERN 3.8.2 XSD validation failed");
    Err(Error::PolicyGate("ERN_XSD_INVALID"))
}

fn wait_status_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    // Reap the zombie: kill alone leaves it unreaped until
                    // the Child handle is dropped, and drop does not wait.
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_files_are_vendored() {
        assert!(
            schema_path().is_file(),
            "vendored ERN 3.8.2 XSD missing at {}",
            schema_path().display()
        );
        assert!(
            schema_path()
                .parent()
                .unwrap()
                .join("avs_20161006.xsd")
                .is_file()
        );
    }

    #[test]
    fn malformed_xml_is_rejected() {
        let err = validate_ern_382_xml("<ern:NewReleaseMessage>").unwrap_err();
        assert!(
            matches!(
                err,
                Error::PolicyGate("ERN_XSD_INVALID")
                    | Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE")
            ),
            "unexpected error: {err:?}"
        );
    }
}
