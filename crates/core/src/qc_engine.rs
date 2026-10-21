//! AUDENIQ's own lossless audio engine (`audeniq-music qc`), run as a
//! sandboxed child process. It replaces FFmpeg/FFprobe for every production media path:
//! probing, lossless FLAC normalization, the Stage 1 QC decode with its
//! fingerprint windows, cover-image probing and the audio tag fallback.
//!
//! The engine is never linked into the API or worker: uploaded bytes are
//! parsed only inside [`crate::parser_sandbox::restrict_audio`] (Landlock,
//! seccomp, cleared environment, RLIMIT_AS), with the parent enforcing the
//! wall deadline, kill/reap and bounded output. Integration contract:
//! <https://github.com/TAE-OK-11/audeniq-media/blob/main/docs/audio/INTEGRATION.md>.
use serde::Deserialize;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Loudness/peak metric definition the Stage 1 rules were calibrated on.
/// A different engine metric is a different measurement: refuse it rather
/// than silently mixing results under one QC rule version.
pub const METRIC_VERSION: &str = "native-bs1770-v1";
/// Fingerprint PCM resampler; tied to [`crate::fingerprint::FINGERPRINT_VERSION`].
pub const RESAMPLER_VERSION: &str = "native-sinc64-v1";

/// Upper bound on a probe/tags/convert report. Reports are small; a larger
/// one is a broken engine, not data to parse.
const SMALL_REPORT_MAX: u64 = 1024 * 1024;
/// Analysis reports carry the 50 ms block envelope (<= 36 000 values) and up
/// to 3 x 30 s of 11025 Hz fingerprint PCM as JSON integers (~7 MB).
const ANALYSIS_REPORT_MAX: u64 = 32 * 1024 * 1024;
const STDERR_MAX: u64 = 4096;

/// The AUDENIQ media engine executable (`audeniq-music`, built at the commit
/// pinned in deploy/audeniq-music.rev): audio commands run as
/// `audeniq-music qc ...`, image/PDF commands as `audeniq-music photo ...`
/// ([`crate::photo`]). The sandbox replaces `PATH`, so the default is absolute.
pub fn bin() -> String {
    std::env::var("AUDENIQ_MUSIC_BIN").unwrap_or_else(|_| "/usr/local/bin/audeniq-music".into())
}

/// Why the engine refused the input (its stable `error` code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// Malformed, corrupted or truncated media.
    Invalid,
    /// Well-formed but outside the engine's lossless contract (lossy codec,
    /// float/32-bit PCM, >2 channels, unsupported container).
    Unsupported,
    /// Exceeds the engine's file, frame or packet limits.
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The engine could not run to completion (missing binary, sandbox,
    /// deadline, signal, local IO). Transient from the file's point of view.
    Unavailable,
    /// The engine's own deadline expired.
    Deadline,
    /// The engine ran and rejected these bytes. Deterministic.
    Rejected(Rejection, String),
}

impl EngineError {
    pub fn is_rejection(&self) -> bool {
        matches!(self, Self::Rejected(..))
    }
}

/// Output/input spec the engine reports for decoded audio.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Spec {
    pub container: String,
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub bits_per_sample: u32,
    pub frames: u64,
}

/// One fingerprint window: mono 11025 Hz s16 PCM from the continuous
/// resampler, starting at `start_secs` of the track.
#[derive(Debug, Clone, Deserialize)]
pub struct FingerprintWindow {
    pub start_secs: f64,
    pub samples: Vec<i16>,
}

/// `analyze` report (also nested in `convert --analyze`).
#[derive(Debug, Clone, Deserialize)]
pub struct Analysis {
    pub metric_version: String,
    pub spec: Spec,
    pub samples_per_channel: u64,
    pub peak: f32,
    pub channel_peaks: Vec<f32>,
    pub integrated_lufs: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub clip_events: u64,
    pub clipped_samples: u64,
    pub blocks: u64,
    pub silent_blocks: u64,
    pub longest_silent_run: u64,
    pub zero_crossing_rate: f64,
    pub block_db: Vec<f32>,
    pub pcm_sha256: String,
    #[serde(default)]
    pub resampler_version: Option<String>,
    #[serde(default)]
    pub fingerprint_windows: Vec<FingerprintWindow>,
}

/// `fingerprint` report.
#[derive(Debug, Clone, Deserialize)]
pub struct FingerprintReport {
    pub resampler_version: String,
    pub spec: Spec,
    pub fingerprint_windows: Vec<FingerprintWindow>,
}

/// `convert` report: `spec` is the verified output FLAC.
#[derive(Debug, Clone, Deserialize)]
pub struct Conversion {
    pub spec: Spec,
    pub frames: u64,
    pub pcm_sha256: String,
    pub output_bytes: u64,
    /// With `--analyze`: the same report `analyze` gives (without
    /// fingerprint windows), measured on the source PCM during the
    /// conversion's own decode.
    #[serde(default)]
    pub analysis: Option<Analysis>,
}

#[derive(Deserialize)]
struct ErrorReport {
    error: String,
    #[serde(default)]
    detail: String,
}

fn classify(stderr: &[u8]) -> EngineError {
    let Ok(report) = serde_json::from_slice::<ErrorReport>(stderr.trim_ascii()) else {
        return EngineError::Unavailable;
    };
    let detail = report.detail;
    match report.error.as_str() {
        "INVALID_INPUT" => EngineError::Rejected(Rejection::Invalid, detail),
        "UNSUPPORTED" => EngineError::Rejected(Rejection::Unsupported, detail),
        "RESOURCE_LIMIT" => EngineError::Rejected(Rejection::Limit, detail),
        "DEADLINE" => EngineError::Deadline,
        // A short read is the file ending early (truncated upload), not a
        // local IO fault: retrying the same bytes cannot succeed.
        "IO" if detail.contains("unexpected end of file")
            || detail.contains("failed to fill whole buffer") =>
        {
            EngineError::Rejected(Rejection::Invalid, detail)
        }
        _ => EngineError::Unavailable,
    }
}

/// Run one engine command in the audio parser sandbox and return its stdout.
/// `outputs` are the only writable grants (a private staging directory).
fn run(
    args: &[&std::ffi::OsStr],
    inputs: &[&Path],
    outputs: &[&Path],
    timeout: Duration,
    stdout_max: u64,
) -> Result<Vec<u8>, EngineError> {
    run_bin(&bin(), args, inputs, outputs, timeout, stdout_max)
}

fn run_bin(
    bin: &str,
    args: &[&std::ffi::OsStr],
    inputs: &[&Path],
    outputs: &[&Path],
    timeout: Duration,
    stdout_max: u64,
) -> Result<Vec<u8>, EngineError> {
    use EngineError::Unavailable;
    let deadline = Instant::now().checked_add(timeout).ok_or(Unavailable)?;
    let mut command = Command::new(bin);
    // The engine checks its own deadline between packets; the parent still
    // enforces the wall clock and kills a stuck child.
    command
        .arg("qc")
        .arg("--timeout-secs")
        .arg(timeout.as_secs().clamp(1, 3600).to_string())
        .args(args);
    crate::parser_sandbox::restrict_audio(&mut command, inputs, outputs)
        .map_err(|_| Unavailable)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::analyzer_process::AnalyzerProcess::spawn(&mut command, deadline)
        .map_err(|_| Unavailable)?;
    let stdout = child.stdout.take().ok_or(Unavailable)?;
    let stdout = child.reader(stdout).map_err(|_| Unavailable)?;
    let stderr = child.stderr.take().ok_or(Unavailable)?;
    let stderr = child.reader(stderr).map_err(|_| Unavailable)?;
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    child
        .spawn_reader("qc-engine-out", move || {
            let mut buf = Vec::new();
            let r = stdout
                .take(stdout_max + 1)
                .read_to_end(&mut buf)
                .map(|_| buf);
            let _ = out_tx.send(r);
        })
        .map_err(|_| Unavailable)?;
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    child
        .spawn_reader("qc-engine-err", move || {
            let mut buf = Vec::new();
            let _ = stderr.take(STDERR_MAX).read_to_end(&mut buf);
            let _ = err_tx.send(buf);
        })
        .map_err(|_| Unavailable)?;
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let out = match out_rx.recv_timeout(remaining()) {
        Ok(Ok(out)) if out.len() as u64 <= stdout_max => out,
        // Oversized report or a timed-out read: Drop kills the process group.
        _ => return Err(Unavailable),
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => return Err(Unavailable),
        }
    };
    let err = err_rx.recv_timeout(remaining()).unwrap_or_default();
    if status.success() {
        return Ok(out);
    }
    // Exit 2 carries a JSON error report; anything else (signal, exec
    // failure, allocation limit abort) says nothing about the bytes.
    if status.code() == Some(2) {
        Err(classify(&err))
    } else {
        Err(Unavailable)
    }
}

fn parse<T: serde::de::DeserializeOwned>(out: &[u8]) -> Result<T, EngineError> {
    // A successful exit with an unreadable report is an engine fault.
    serde_json::from_slice(out).map_err(|_| EngineError::Unavailable)
}

/// FFprobe-compatible metadata report (`streams` / `format`), for audio or a
/// JPEG/PNG cover. Metadata only: it is not proof the whole file decodes.
pub fn probe(path: &Path, timeout: Duration) -> Result<serde_json::Value, EngineError> {
    probe_with(&bin(), path, timeout)
}

pub(crate) fn probe_with(
    bin: &str,
    path: &Path,
    timeout: Duration,
) -> Result<serde_json::Value, EngineError> {
    let out = run_bin(
        bin,
        &["probe".as_ref(), path.as_os_str()],
        &[path],
        &[],
        timeout,
        SMALL_REPORT_MAX,
    )?;
    parse(&out)
}

/// Purpose-specific audio tags in the FFprobe `format.tags` shape.
pub fn tags(path: &Path, timeout: Duration) -> Result<serde_json::Value, EngineError> {
    let out = run(
        &["tags".as_ref(), path.as_os_str()],
        &[path],
        &[],
        timeout,
        SMALL_REPORT_MAX,
    )?;
    parse(&out)
}

/// One full streaming decode: QC metrics, canonical PCM SHA-256 and, with
/// `fingerprint`, the head/middle/tail fingerprint windows.
pub fn analyze(path: &Path, fingerprint: bool, timeout: Duration) -> Result<Analysis, EngineError> {
    let mut args: Vec<&std::ffi::OsStr> = vec!["analyze".as_ref(), path.as_os_str()];
    if fingerprint {
        args.push("--fingerprint".as_ref());
    }
    let out = run(&args, &[path], &[], timeout, ANALYSIS_REPORT_MAX)?;
    let analysis: Analysis = parse(&out)?;
    check_versions(
        &analysis.metric_version,
        analysis.resampler_version.as_deref(),
        fingerprint,
    )?;
    Ok(analysis)
}

/// Canonical PCM SHA-256 (interleaved, left-aligned s32le) of a full decode.
pub fn pcm_sha256(path: &Path, timeout: Duration) -> Result<String, EngineError> {
    #[derive(Deserialize)]
    struct Report {
        pcm_sha256: String,
    }
    let out = run(
        &["pcm-hash".as_ref(), path.as_os_str()],
        &[path],
        &[],
        timeout,
        SMALL_REPORT_MAX,
    )?;
    Ok(parse::<Report>(&out)?.pcm_sha256)
}

/// Fingerprint windows only (no QC metrics).
pub fn fingerprint(path: &Path, timeout: Duration) -> Result<FingerprintReport, EngineError> {
    let out = run(
        &["fingerprint".as_ref(), path.as_os_str()],
        &[path],
        &[],
        timeout,
        ANALYSIS_REPORT_MAX,
    )?;
    let report: FingerprintReport = parse(&out)?;
    check_versions(METRIC_VERSION, Some(&report.resampler_version), true)?;
    Ok(report)
}

fn check_versions(
    metric: &str,
    resampler: Option<&str>,
    fingerprint: bool,
) -> Result<(), EngineError> {
    if metric != METRIC_VERSION || (fingerprint && resampler != Some(RESAMPLER_VERSION)) {
        tracing::error!(
            metric,
            resampler,
            "audeniq-music qc engine version does not match the QC rule/fingerprint version"
        );
        return Err(EngineError::Unavailable);
    }
    Ok(())
}

/// Lossless normalization to FLAC. `staging` must be a new, private, empty
/// directory: it is the engine's only writable grant. The engine writes a
/// temporary sibling, re-decodes and hashes it against the source PCM, and
/// publishes `staging/<name>` with a no-clobber link only after that check.
///
/// With `analyze`, the engine also measures the source PCM in the same
/// decode (`--analyze`, the `analyze` report under `analysis`), for the
/// upload's immediate QC answer. The analysis is dropped (`None`) when its
/// metric version does not match the QC rules or its PCM hash does not
/// match the conversion's.
pub fn convert(
    src: &Path,
    staging: &Path,
    name: &str,
    timeout: Duration,
    analyze: bool,
) -> Result<(std::path::PathBuf, Conversion), EngineError> {
    let dst = staging.join(name);
    let mut args: Vec<&std::ffi::OsStr> =
        vec!["convert".as_ref(), src.as_os_str(), dst.as_os_str()];
    if analyze {
        args.push("--analyze".as_ref());
    }
    let out = run(
        &args,
        &[src],
        &[staging],
        timeout,
        if analyze {
            ANALYSIS_REPORT_MAX
        } else {
            SMALL_REPORT_MAX
        },
    )?;
    let mut conversion: Conversion = parse(&out)?;
    conversion.analysis = conversion.analysis.take().filter(|a| {
        a.pcm_sha256 == conversion.pcm_sha256
            && check_versions(&a.metric_version, a.resampler_version.as_deref(), false).is_ok()
    });
    Ok((dst, conversion))
}

/// Mono 11025 Hz fingerprint windows as the f32 segments the backend's
/// fingerprint core consumes.
pub fn windows_to_segments(windows: &[FingerprintWindow]) -> Vec<Vec<f32>> {
    windows
        .iter()
        .filter(|w| !w.samples.is_empty())
        .map(|w| w.samples.iter().map(|s| f32::from(*s) / 32768.0).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_error_reports_are_classified() {
        let r = |s: &str| classify(s.as_bytes());
        assert_eq!(
            r(r#"{"error":"UNSUPPORTED","detail":"unsupported: ALAC audio track only"}"#),
            EngineError::Rejected(
                Rejection::Unsupported,
                "unsupported: ALAC audio track only".into()
            )
        );
        assert!(matches!(
            r(r#"{"error":"INVALID_INPUT","detail":"x"}"#),
            EngineError::Rejected(Rejection::Invalid, _)
        ));
        assert!(matches!(
            r(r#"{"error":"RESOURCE_LIMIT","detail":"x"}"#),
            EngineError::Rejected(Rejection::Limit, _)
        ));
        assert!(matches!(
            r(r#"{"detail":"I/O: unexpected end of file","error":"IO"}"#),
            EngineError::Rejected(Rejection::Invalid, _)
        ));
        assert_eq!(
            r(r#"{"detail":"I/O: Permission denied (os error 13)","error":"IO"}"#),
            EngineError::Unavailable
        );
        assert_eq!(
            r(r#"{"error":"DEADLINE","detail":""}"#),
            EngineError::Deadline
        );
        assert_eq!(r("Segmentation fault"), EngineError::Unavailable);
        assert_eq!(r(""), EngineError::Unavailable);
    }

    #[test]
    fn missing_engine_is_unavailable_not_a_rejection() {
        let err = run(
            &["capabilities".as_ref()],
            &[],
            &[],
            Duration::from_secs(5),
            SMALL_REPORT_MAX,
        );
        // With the real engine on PATH this succeeds; a missing one must
        // never be reported as a defect of the user's file.
        if let Err(e) = err {
            assert!(!e.is_rejection(), "{e:?}");
        }
    }

    #[test]
    fn version_mismatch_is_refused() {
        assert!(check_versions(METRIC_VERSION, Some(RESAMPLER_VERSION), true).is_ok());
        assert!(check_versions(METRIC_VERSION, None, false).is_ok());
        assert_eq!(
            check_versions("native-bs1770-v2", None, false),
            Err(EngineError::Unavailable)
        );
        assert_eq!(
            check_versions(METRIC_VERSION, Some("native-sinc64-v2"), true),
            Err(EngineError::Unavailable)
        );
    }
}
