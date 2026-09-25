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
    let doc_path = std::env::temp_dir().join(format!(
        "audeniq-ern-{}-{}.xml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&doc_path, xml.as_bytes())
        .map_err(|_| Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
    let result = run_xmllint(&schema, &doc_path);
    let _ = std::fs::remove_file(&doc_path);
    result
}

fn run_xmllint(schema: &std::path::Path, doc: &std::path::Path) -> Result<()> {
    let mut child = Command::new("xmllint")
        .args(["--noout", "--schema"])
        .arg(schema)
        .arg(doc)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
    // xmllint on a local document returns quickly; guard against a hung
    // validator the same way the QC pipeline guards ffmpeg.
    let output = wait_with_timeout(&mut child, Duration::from_secs(60))
        .ok_or(Error::PolicyGate("ERN_XSD_VALIDATOR_UNAVAILABLE"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail: String = stderr
        .lines()
        .take(8)
        .collect::<Vec<_>>()
        .join(" | ")
        .chars()
        .take(500)
        .collect();
    // The gate code stays a plain PolicyGate so the API error surface is
    // unchanged; the validator diagnostics go to the log, never the client.
    tracing::warn!(gate = "ERN_XSD_INVALID", detail = %detail, "ERN 3.8.2 XSD validation failed");
    Err(Error::PolicyGate("ERN_XSD_INVALID"))
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    use std::io::Read;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // The child has exited; drain the pipes without taking
                // ownership (wait_with_output needs `self` by value).
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(out) = child.stdout.as_mut() {
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(err) = child.stderr.as_mut() {
                    let _ = err.read_to_end(&mut stderr);
                }
                return Some(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
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
