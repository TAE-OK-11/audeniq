//! F2 Stage 1 file QC (BLUEPRINT §4 1-C).
//!
//! Pure analysis over local files: magic-byte verification plus `ffprobe`
//! metrics. No rights judgement, no personal data. Rule changes bump
//! [`QC_RULE_VERSION`] so cached results stay valid per version.
use crate::error::{Error, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// Bump when any threshold, check set, or metric definition changes.
pub const QC_RULE_VERSION: &str = "3";

/// Minimum audio duration in seconds before flagging as suspiciously short.
pub const MIN_AUDIO_SECS: f64 = 30.0;
/// Minimum sample rate accepted without correction.
pub const MIN_SAMPLE_RATE: u32 = 44_100;
/// Highest accepted sample rate (the DSP hi-res ceiling).
pub const MAX_SAMPLE_RATE: u32 = 192_000;
/// Spotify floor: below 16-bit is upconverted but ineligible for lossless.
pub const MIN_BIT_DEPTH: u32 = 16;
/// Minimum long-side pixels for cover art.
pub const MIN_IMAGE_LONG_SIDE: u32 = 3000;
/// Upper bound on ffprobe JSON output we will parse. A corrupt file must not
/// OOM the worker through unbounded analyzer output; larger output fails
/// closed and is retried as a technical failure.
pub const PROBE_OUTPUT_MAX: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    CorrectionRequired,
    ReviewRequired,
    Blocked,
    TechnicalRetry,
    NotApplicable,
}

impl CheckStatus {
    pub fn as_db(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::CorrectionRequired => "CORRECTION_REQUIRED",
            Self::ReviewRequired => "REVIEW_REQUIRED",
            Self::Blocked => "BLOCKED",
            Self::TechnicalRetry => "TECHNICAL_RETRY",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub check_code: &'static str,
    pub status: CheckStatus,
    /// Stable inputs that produced this outcome (metric hash), for the change cache.
    pub input_hash: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AudioMetrics {
    pub format_name: String,
    pub codec_name: String,
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u32,
    /// Effective PCM bit depth: `bits_per_sample` for WAV, and
    /// `bits_per_raw_sample` for FLAC (ffprobe reports 0 for FLAC's
    /// `bits_per_sample`, which previously let an 8-bit FLAC pass).
    pub bits_per_sample: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImageMetrics {
    pub format_name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize)]
struct ProbeJson {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: ProbeFormat,
}
#[derive(Deserialize, Default)]
struct ProbeStream {
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    bits_per_raw_sample: String,
    #[serde(default)]
    sample_rate: String,
    #[serde(default)]
    channels: u32,
    #[serde(default)]
    bits_per_sample: u32,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
}
#[derive(Deserialize, Default)]
struct ProbeFormat {
    #[serde(default)]
    format_name: String,
    #[serde(default)]
    duration: String,
}

fn ffprobe_bin() -> String {
    std::env::var("FFPROBE_BIN").unwrap_or_else(|_| "ffprobe".into())
}

fn probe_timeout() -> Duration {
    std::env::var("AUDENIQ_QC_PROBE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(30))
}

/// Why an analyzer run failed. The distinction decides between a retry and
/// a user-visible correction: retrying the same bytes after `Undecodable`
/// can never succeed, so it must not burn attempts until the job dies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzerError {
    /// The analyzer could not run to completion (binary missing, spawn
    /// failure, timeout, IO). Transient from the file's point of view.
    Unavailable,
    /// The analyzer ran and rejected the file (non-zero exit, no audio
    /// stream, unparseable report). Deterministic for these bytes.
    Undecodable,
}

/// Run ffprobe and parse its JSON report. Any failure (missing binary,
/// undecodable file, timeout, oversized output) is an error.
pub fn probe(path: &Path) -> Result<serde_json::Value> {
    probe_with(&ffprobe_bin(), path, probe_timeout()).map_err(|_| Error::Internal)
}

/// [`probe`] with the failure classified (see [`AnalyzerError`]).
pub fn probe_classified(path: &Path) -> std::result::Result<serde_json::Value, AnalyzerError> {
    probe_with(&ffprobe_bin(), path, probe_timeout())
}

fn probe_with(
    bin: &str,
    path: &Path,
    timeout: Duration,
) -> std::result::Result<serde_json::Value, AnalyzerError> {
    use AnalyzerError::{Unavailable, Undecodable};
    let mut child = std::process::Command::new(bin)
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            &path.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Unavailable)?;
    let stdout = child.stdout.take().ok_or(Unavailable)?;
    // Bounded read in a helper thread; the main thread enforces the timeout
    // and kills a hung analyzer so one corrupt file cannot wedge the worker.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stdout
            .take(PROBE_OUTPUT_MAX + 1)
            .read_to_end(&mut buf)
            .map(|_| buf);
        let _ = tx.send(r);
    });
    let buf = match rx.recv_timeout(timeout) {
        Ok(Ok(buf)) => buf,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Unavailable);
        }
    };
    if buf.len() as u64 > PROBE_OUTPUT_MAX {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Undecodable);
    }
    let status = child.wait().map_err(|_| Unavailable)?;
    if !status.success() {
        return Err(Undecodable);
    }
    serde_json::from_slice(&buf).map_err(|_| Undecodable)
}

fn parse_audio(v: &serde_json::Value) -> Result<AudioMetrics> {
    let p: ProbeJson = serde_json::from_value(v.clone()).map_err(|_| Error::Internal)?;
    let s = p
        .streams
        .iter()
        .find(|s| s.codec_type == "audio")
        .ok_or(Error::Internal)?;
    let raw_bits: u32 = s.bits_per_raw_sample.parse().unwrap_or(0);
    Ok(AudioMetrics {
        format_name: p.format.format_name.split(',').next().unwrap_or("").into(),
        codec_name: s.codec_name.clone(),
        duration_secs: p.format.duration.parse().unwrap_or(0.0),
        sample_rate: s.sample_rate.parse().unwrap_or(0),
        channels: s.channels,
        bits_per_sample: if raw_bits > 0 {
            Some(raw_bits)
        } else if s.bits_per_sample > 0 {
            Some(s.bits_per_sample)
        } else {
            None
        },
    })
}

fn parse_image(v: &serde_json::Value) -> Result<ImageMetrics> {
    let p: ProbeJson = serde_json::from_value(v.clone()).map_err(|_| Error::Internal)?;
    let s = p
        .streams
        .iter()
        .find(|s| s.codec_type == "video" || s.codec_type == "image")
        .ok_or(Error::Internal)?;
    Ok(ImageMetrics {
        format_name: p.format.format_name.split(',').next().unwrap_or("").into(),
        width: s.width,
        height: s.height,
    })
}

/// Measured audio duration in seconds via ffprobe. Returns `None` when the
/// file cannot be probed or has no parseable duration. Used by Stage 1 to
/// persist `catalog.assets.duration_secs`, which the DDEX ERN builder needs
/// for the schema-required `SoundRecording/Duration` element.
pub fn probe_duration_secs(path: &Path) -> Option<f64> {
    probe_audio_metrics(path).map(|m| m.duration_secs)
}

/// Measured audio technical metrics via ffprobe: duration plus the real
/// sample rate / channel count / bits per sample. Returns `None` when the
/// file cannot be probed or has no audio stream. Stage 1 persists these to
/// `catalog.assets.{sample_rate,channels,bits_per_sample}` so the DDEX ERN
/// builder can emit the true `TechnicalSoundRecordingDetails` instead of
/// fabricated constants.
pub fn probe_audio_metrics(path: &Path) -> Option<AudioMetrics> {
    let v = probe(path).ok()?;
    let m = parse_audio(&v).ok()?;
    // A non-positive or non-finite duration is not a measurement; the DDEX
    // builder fails closed without one (DDEX_DURATION_UNKNOWN).
    (m.sample_rate > 0 && m.channels > 0 && m.duration_secs.is_finite() && m.duration_secs > 0.0)
        .then_some(m)
}

/// Detect container from magic bytes. Returns a canonical tag or "UNKNOWN".
pub fn detect_container(head: &[u8]) -> &'static str {
    // RF64/BW64 are the EBU >4 GiB WAV variants (ffmpeg reads them as "wav").
    if head.len() >= 12
        && matches!(&head[0..4], b"RIFF" | b"RF64" | b"BW64")
        && &head[8..12] == b"WAVE"
    {
        "WAV"
    } else if head.len() >= 4 && &head[0..4] == b"fLaC" {
        "FLAC"
    } else if head.len() >= 3
        && (&head[0..3] == b"ID3" || (head[0] == 0xFF && head[1] & 0xE0 == 0xE0))
    {
        "MP3"
    } else if head.len() >= 8 && &head[0..8] == b"\x89PNG\r\n\x1a\n" {
        "PNG"
    } else if head.len() >= 3 && &head[0..3] == b"\xFF\xD8\xFF" {
        "JPEG"
    } else {
        "UNKNOWN"
    }
}

fn metric_hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update(b"\x00");
    }
    hex::encode(h.finalize())
}

/// Cache key: identical check + rule version + asset bytes + metrics ⇒ identical outcome.
pub fn result_hash(
    check_code: &str,
    rule_version: &str,
    asset_sha256: &str,
    input_hash: &str,
) -> String {
    metric_hash(&[check_code, rule_version, asset_sha256, input_hash])
}

/// Streaming SHA-256 of a local file (constant memory for 512 MiB masters).
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path).map_err(|_| Error::Internal)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|_| Error::Internal)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

fn head_bytes(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|_| Error::Internal)?;
    let mut buf = vec![0u8; 16];
    let n = f.read(&mut buf).map_err(|_| Error::Internal)?;
    buf.truncate(n);
    Ok(buf)
}

/// Fixed Stage 1 audio check contract: every analyzed audio asset yields exactly
/// these outcomes, in order. Later stages and the Studio UI can rely on the
/// set being stable; checks that could not run are NOT_APPLICABLE or
/// TECHNICAL_RETRY instead of being silently dropped. The policy behind each
/// threshold is documented in `docs/AUDIO_QC_POLICY.md`.
pub const AUDIO_CHECK_CODES: &[&str] = &[
    "SHA256_MISMATCH",
    "AUDIO_MAGIC_MISMATCH",
    "AUDIO_PROBE_FAILED",
    "AUDIO_SAMPLE_FORMAT_UNSUPPORTED",
    "AUDIO_TOO_SHORT",
    "AUDIO_SAMPLE_RATE_LOW",
    "AUDIO_BIT_DEPTH_LOW",
    "AUDIO_CHANNEL_INVALID",
    "AUDIO_TRUNCATED",
    "AUDIO_SILENT",
    "AUDIO_CLIPPING",
    "AUDIO_LOUDNESS_OUT_OF_RANGE",
    "AUDIO_CONTENT_SUSPECT",
    // Perceptual similarity is DB-backed and computed in submission.rs, not
    // in check_audio: the codes live here so the fixed contract (tails,
    // caching, counts) covers them, but check_audio only emits them via
    // tail() short-circuits. The real outcomes are produced by
    // submission::handle_fingerprint_checks.
    "AUDIO_FINGERPRINT_FAILED",
    "AUDIO_SIMILAR_TO_EXISTING",
];

fn audio_code_index(code: &str) -> usize {
    AUDIO_CHECK_CODES
        .iter()
        .position(|c| *c == code)
        .expect("code is part of the audio contract")
}

/// A sample at or above this absolute level (about -0.009 dBFS) is at digital
/// full scale.
pub const CLIP_LEVEL: f32 = 0.999;
/// Consecutive full-scale samples (per channel) that make one clip event.
/// Single full-scale samples occur in legitimately peak-normalized masters;
/// three or more in a row are a flattened waveform.
pub const CLIP_RUN: u32 = 3;
/// Share of samples inside clip events at which the master is rejected
/// (0.1 %). Below this, clip events are recorded as a review-only warning.
pub const CLIP_REJECT_RATIO: f64 = 0.001;
/// Whole-file peak below this (-80 dBFS) is treated as digital silence.
pub const SILENCE_PEAK: f32 = 1e-4;
/// Truncation tolerance: decoded audio may fall short of the header's
/// promise by at most max(1 s, 1 %) before the file counts as truncated.
pub const TRUNCATION_TOLERANCE_SECS: f64 = 1.0;
pub const TRUNCATION_TOLERANCE_RATIO: f64 = 0.01;

/// Stage 1 basic QC for an audio asset file.
///
/// `declared_content_type` is the MIME type the upload was registered with;
/// when given, the real container must match it.
/// Thin wrapper keeping the historical `check_audio` contract for tests.
pub fn check_audio(
    path: &Path,
    registered_sha256: Option<&str>,
    declared_content_type: Option<&str>,
) -> Vec<CheckOutcome> {
    check_audio_full(path, registered_sha256, declared_content_type).0
}

/// Full audio check contract plus the fingerprint tap samples collected
/// during the single `decode_analysis` pass (mono 11025 Hz PCM over the
/// [`crate::fingerprint::segment_windows`]; empty when the bytes were
/// rejected before decoding).
pub fn check_audio_full(
    path: &Path,
    registered_sha256: Option<&str>,
    declared_content_type: Option<&str>,
) -> (Vec<CheckOutcome>, Vec<f32>) {
    /// Emit `AUDIO_CHECK_CODES[from..]` with a uniform status (short-circuit tail).
    fn tail(from: &str, status: CheckStatus, input_hash: &str, detail: &str) -> Vec<CheckOutcome> {
        AUDIO_CHECK_CODES[audio_code_index(from)..]
            .iter()
            .map(|code| CheckOutcome {
                check_code: code,
                status,
                input_hash: input_hash.to_string(),
                detail: detail.into(),
            })
            .collect()
    }
    fn outcome(
        code: &'static str,
        ok: bool,
        bad: CheckStatus,
        ih: &str,
        detail: String,
    ) -> CheckOutcome {
        CheckOutcome {
            check_code: code,
            status: if ok { CheckStatus::Pass } else { bad },
            input_hash: ih.to_string(),
            detail,
        }
    }
    let actual = match sha256_file(path) {
        Ok(h) => h,
        Err(_) => {
            return (
                tail(
                    "SHA256_MISMATCH",
                    CheckStatus::TechnicalRetry,
                    &metric_hash(&["unreadable"]),
                    "cannot read file",
                ),
                Vec::new(),
            );
        }
    };
    let mut out = Vec::with_capacity(AUDIO_CHECK_CODES.len());
    let sha_status = match registered_sha256 {
        Some(expected) if expected == actual => CheckStatus::Pass,
        Some(_) => CheckStatus::Blocked,
        None => CheckStatus::Pass,
    };
    out.push(CheckOutcome {
        check_code: "SHA256_MISMATCH",
        status: sha_status,
        input_hash: metric_hash(&[&actual]),
        detail: match registered_sha256 {
            Some(expected) => format!("registered={expected} actual={actual}"),
            None => "no registered hash".into(),
        },
    });
    if sha_status == CheckStatus::Blocked {
        // Bytes differ from what was admitted: do not analyze the wrong bytes.
        out.extend(tail(
            "AUDIO_MAGIC_MISMATCH",
            CheckStatus::NotApplicable,
            &metric_hash(&[&actual]),
            "sha256 mismatch",
        ));
        return (out, Vec::new());
    }
    let head = head_bytes(path).unwrap_or_default();
    let container = detect_container(&head);
    let declared =
        declared_content_type.and_then(|ct| crate::uploads::expected_container("AUDIO", ct));
    let (magic_ok, magic_detail) = match (container, declared) {
        ("MP3", _) => (
            false,
            "detected=MP3: lossy MP3 cannot be distributed as a master; upload the original WAV or FLAC".to_string(),
        ),
        ("WAV" | "FLAC", Some(d)) if d != container => (
            false,
            format!(
                "detected={container} declared={}: the file content does not match its declared type",
                declared_content_type.unwrap_or("")
            ),
        ),
        ("WAV" | "FLAC", _) => (true, format!("detected={container}")),
        _ => (
            false,
            format!("detected={container}: not a WAV or FLAC file"),
        ),
    };
    let magic_hash = metric_hash(&[container, declared.unwrap_or("")]);
    out.push(outcome(
        "AUDIO_MAGIC_MISMATCH",
        magic_ok,
        CheckStatus::CorrectionRequired,
        &magic_hash,
        magic_detail,
    ));
    if !magic_ok {
        out.extend(tail(
            "AUDIO_PROBE_FAILED",
            CheckStatus::NotApplicable,
            &magic_hash,
            "not an accepted audio container",
        ));
        return (out, Vec::new());
    }
    let metrics = match probe_classified(path)
        .and_then(|v| parse_audio(&v).map_err(|_| AnalyzerError::Undecodable))
    {
        Ok(m) => m,
        Err(e) => {
            let (status, detail) = match e {
                // The analyzer ran and rejected the bytes (e.g. a corrupted
                // header behind a valid RIFF/WAVE signature): retrying cannot
                // help, the user must export the file again.
                AnalyzerError::Undecodable => (
                    CheckStatus::CorrectionRequired,
                    "the file has a valid signature but its audio data cannot be read (corrupted or incomplete header); export the master again",
                ),
                AnalyzerError::Unavailable => (
                    CheckStatus::TechnicalRetry,
                    "ffprobe unavailable or timed out",
                ),
            };
            out.push(CheckOutcome {
                check_code: "AUDIO_PROBE_FAILED",
                status,
                input_hash: metric_hash(&[&actual]),
                detail: detail.into(),
            });
            out.extend(tail(
                "AUDIO_SAMPLE_FORMAT_UNSUPPORTED",
                if status == CheckStatus::TechnicalRetry {
                    CheckStatus::TechnicalRetry
                } else {
                    CheckStatus::NotApplicable
                },
                &metric_hash(&[&actual]),
                "probe failed",
            ));
            return (out, Vec::new());
        }
    };
    out.push(CheckOutcome {
        check_code: "AUDIO_PROBE_FAILED",
        status: CheckStatus::Pass,
        input_hash: metric_hash(&[&actual]),
        detail: "ffprobe ok".into(),
    });
    let mh = metric_hash(&[
        &metrics.format_name,
        &metrics.codec_name,
        &metrics.duration_secs.to_string(),
        &metrics.sample_rate.to_string(),
        &metrics.channels.to_string(),
        &metrics
            .bits_per_sample
            .map(|b| b.to_string())
            .unwrap_or_default(),
    ]);
    let (format_ok, format_detail) = sample_format_policy(container, &metrics);
    out.push(outcome(
        "AUDIO_SAMPLE_FORMAT_UNSUPPORTED",
        format_ok,
        CheckStatus::CorrectionRequired,
        &mh,
        format_detail,
    ));
    out.push(outcome(
        "AUDIO_TOO_SHORT",
        metrics.duration_secs >= MIN_AUDIO_SECS,
        CheckStatus::CorrectionRequired,
        &mh,
        format!("duration_secs={:.2}", metrics.duration_secs),
    ));
    out.push(outcome(
        "AUDIO_SAMPLE_RATE_LOW",
        metrics.sample_rate >= MIN_SAMPLE_RATE,
        CheckStatus::CorrectionRequired,
        &mh,
        format!("sample_rate={}", metrics.sample_rate),
    ));
    // Spotify: below 16-bit is upconverted but ineligible for lossless.
    // Unknown bit depth (probe didn't report it) is not a failure — the
    // checks we can't verify, we don't block on.
    out.push(outcome(
        "AUDIO_BIT_DEPTH_LOW",
        !matches!(metrics.bits_per_sample, Some(b) if b < MIN_BIT_DEPTH),
        CheckStatus::CorrectionRequired,
        &mh,
        format!("bits_per_sample={:?}", metrics.bits_per_sample),
    ));
    let channels_ok = metrics.channels == 1 || metrics.channels == 2;
    out.push(outcome(
        "AUDIO_CHANNEL_INVALID",
        channels_ok,
        CheckStatus::CorrectionRequired,
        &mh,
        format!("channels={}", metrics.channels),
    ));
    // Header-level truncation (WAV): the data chunk promises more bytes than
    // the file holds. ffprobe silently reports the shorter real duration, so
    // this must be read from the RIFF structure itself.
    let wav_truncation = if container == "WAV" {
        wav_data_truncation(path)
    } else {
        None
    };
    if !channels_ok || !format_ok {
        // Decoding a 5.1 / 384 kHz / float file to measure it is pointless
        // (and can be very expensive): it is rejected anyway.
        out.extend(tail(
            "AUDIO_TRUNCATED",
            CheckStatus::NotApplicable,
            &mh,
            if channels_ok {
                "sample format rejected"
            } else {
                "channel layout rejected"
            },
        ));
        return (out, Vec::new());
    }
    let (analysis, fp_samples) = match decode_analysis(path, &metrics) {
        Ok((a, s)) => (a, s),
        Err(e) => {
            let (status, detail) = match e {
                AnalyzerError::Undecodable => (
                    CheckStatus::CorrectionRequired,
                    "the audio data could not be decoded to the end (corrupted file); export the master again",
                ),
                AnalyzerError::Unavailable => (
                    CheckStatus::TechnicalRetry,
                    "ffmpeg unavailable or timed out",
                ),
            };
            out.extend(tail("AUDIO_TRUNCATED", status, &mh, detail));
            // Fingerprint codes are produced by the DB-backed step, but only
            // for admitted audio; a decode failure there is already covered.
            return (out, Vec::new());
        }
    };
    let decoded_secs = analysis.decoded_secs();
    let decode_shortfall = metrics.duration_secs - decoded_secs;
    let decode_truncated = decode_shortfall
        > TRUNCATION_TOLERANCE_SECS.max(metrics.duration_secs * TRUNCATION_TOLERANCE_RATIO);
    let (trunc_ok, trunc_detail) = match (wav_truncation, decode_truncated) {
        (Some((declared, available)), _) => (
            false,
            format!(
                "WAV header declares {declared} data bytes but only {available} are present ({:.0}% of the audio); the file is truncated, upload the complete export",
                available as f64 * 100.0 / declared.max(1) as f64
            ),
        ),
        (None, true) => (
            false,
            format!(
                "header declares {:.2}s but only {decoded_secs:.2}s of audio decodes; the file is truncated, upload the complete export",
                metrics.duration_secs
            ),
        ),
        (None, false) => (true, format!("decoded_secs={decoded_secs:.2}")),
    };
    let ah = metric_hash(&[
        &mh,
        &analysis.samples_per_channel.to_string(),
        &format!("{:.6}", analysis.peak),
        &analysis.clipped_samples.to_string(),
        &analysis.clip_events.to_string(),
        &format!("{:?}", analysis.integrated_lufs.map(|v| format!("{v:.1}"))),
        &format!("{:?}", analysis.true_peak_dbtp.map(|v| format!("{v:.1}"))),
        &wav_truncation
            .map(|(d, a)| format!("{d}/{a}"))
            .unwrap_or_default(),
    ]);
    out.push(outcome(
        "AUDIO_TRUNCATED",
        trunc_ok,
        CheckStatus::CorrectionRequired,
        &ah,
        trunc_detail,
    ));
    let silent = analysis.peak < SILENCE_PEAK;
    out.push(outcome(
        "AUDIO_SILENT",
        !silent,
        CheckStatus::CorrectionRequired,
        &ah,
        if silent {
            format!(
                "the whole file is silent (peak {:.1} dBFS); upload the actual recording",
                db(analysis.peak as f64)
            )
        } else {
            format!("peak_dbfs={:.2}", db(analysis.peak as f64))
        },
    ));
    let total_samples = analysis.samples_per_channel * u64::from(metrics.channels.max(1));
    let clip_ratio = analysis.clipped_samples as f64 / total_samples.max(1) as f64;
    let inter_sample_overs = analysis.true_peak_dbtp.is_some_and(|tp| tp > 0.0);
    let clip_status = if clip_ratio >= CLIP_REJECT_RATIO {
        CheckStatus::CorrectionRequired
    } else if analysis.clip_events > 0 || inter_sample_overs {
        CheckStatus::ReviewRequired
    } else {
        CheckStatus::Pass
    };
    out.push(CheckOutcome {
        check_code: "AUDIO_CLIPPING",
        status: clip_status,
        input_hash: ah.clone(),
        detail: match clip_status {
            CheckStatus::CorrectionRequired => format!(
                "heavy clipping: {} clip events, {:.2}% of samples at full scale (limit {:.1}%); re-master with a limiter ceiling below 0 dBFS",
                analysis.clip_events,
                clip_ratio * 100.0,
                CLIP_REJECT_RATIO * 100.0
            ),
            CheckStatus::ReviewRequired => format!(
                "warning (not blocking): {} short clip events ({:.3}% of samples), true peak {}; lossy encoding at DSPs may distort, a limiter ceiling of -1 dBTP is recommended",
                analysis.clip_events,
                clip_ratio * 100.0,
                analysis
                    .true_peak_dbtp
                    .map(|tp| format!("{tp:+.1} dBTP"))
                    .unwrap_or_else(|| "-inf".into())
            ),
            _ => "no clipping detected".into(),
        },
    });
    // Loudness is informational only: DSPs normalize playback loudness, and
    // most commercial masters sit well above -14 LUFS, so an out-of-range
    // measurement is a review-only warning and never blocks a release.
    let (loud_ok, loud_detail) = match (analysis.integrated_lufs, analysis.true_peak_dbtp) {
        (Some(i), Some(tp)) => {
            let in_range = (i - LOUDNESS_TARGET_LUFS).abs() <= LOUDNESS_TOLERANCE_LU
                && tp <= TRUE_PEAK_MAX_DBTP;
            (
                in_range,
                if in_range {
                    format!("integrated_lufs={i:.1} true_peak_dbtp={tp:.1}")
                } else {
                    format!(
                        "warning (not blocking): integrated_lufs={i:.1} true_peak_dbtp={tp:.1}; streaming services normalize to about {LOUDNESS_TARGET_LUFS} LUFS and recommend true peak <= {TRUE_PEAK_MAX_DBTP} dBTP"
                    )
                },
            )
        }
        // -inf LUFS: nothing above the gating threshold. Silence is already
        // reported by AUDIO_SILENT; loudness itself has nothing to add.
        _ => (
            true,
            "integrated_lufs=-inf (no gated programme loudness)".into(),
        ),
    };
    out.push(outcome(
        "AUDIO_LOUDNESS_OUT_OF_RANGE",
        loud_ok,
        CheckStatus::ReviewRequired,
        &ah,
        loud_detail,
    ));
    let suspicions = content_suspicions(&analysis, &metrics);
    out.push(outcome(
        "AUDIO_CONTENT_SUSPECT",
        suspicions.is_empty(),
        CheckStatus::ReviewRequired,
        &metric_hash(&[&ah, &suspicions.join("|")]),
        if suspicions.is_empty() {
            "no near-silence, long silence, dead channel, steady noise or loop pattern".into()
        } else {
            format!("for review (not blocking): {}", suspicions.join("; "))
        },
    ));
    (out, fp_samples)
}

/// Integrated loudness below this is "near-silent" content.
pub const NEAR_SILENT_LUFS: f64 = -45.0;
/// Silence covering this share of the track, or one silent stretch at least
/// this long, is flagged (padding a short clip out to a billable length).
pub const SILENCE_SHARE_REVIEW: f64 = 0.5;
pub const SILENCE_RUN_REVIEW_SECS: f64 = 30.0;
/// A stereo channel this quiet while the other carries audio is "dead".
pub const DEAD_CHANNEL_PEAK: f32 = 0.001;
/// Zero-crossing rate typical of broadband noise (music is far lower).
pub const NOISE_ZCR: f64 = 0.3;

/// Review-only content heuristics (sandbox round 2: near-silent, white
/// noise, one silent channel, a 5 s loop x12, 5 s audio + 45 s silence were
/// all delivered). They never block: they route the release to a human.
pub fn content_suspicions(a: &DecodeAnalysis, m: &AudioMetrics) -> Vec<String> {
    let mut out = Vec::new();
    if a.peak < SILENCE_PEAK {
        return out; // fully silent: AUDIO_SILENT already rejects it
    }
    if let Some(i) = a.integrated_lufs.filter(|i| *i < NEAR_SILENT_LUFS) {
        out.push(format!("near-silent programme ({i:.1} LUFS)"));
    } else if a.integrated_lufs.is_none() {
        out.push("near-silent programme (below the loudness gate)".into());
    }
    let block_secs = BLOCK_SECS;
    let share = a.silent_blocks as f64 / a.blocks.max(1) as f64;
    let run_secs = a.longest_silent_run as f64 * block_secs;
    if share >= SILENCE_SHARE_REVIEW || run_secs >= SILENCE_RUN_REVIEW_SECS {
        out.push(format!(
            "long silence ({:.0}% of the track silent, longest stretch {run_secs:.0}s)",
            share * 100.0
        ));
    }
    if m.channels == 2 && a.channel_peaks.len() == 2 {
        let (l, r) = (a.channel_peaks[0], a.channel_peaks[1]);
        if (l < DEAD_CHANNEL_PEAK) != (r < DEAD_CHANNEL_PEAK) {
            out.push(format!(
                "one channel is silent ({} channel peak {:.0} dBFS)",
                if l < DEAD_CHANNEL_PEAK {
                    "left"
                } else {
                    "right"
                },
                db(f64::from(l.min(r)))
            ));
        }
    }
    let loud: Vec<f32> = a.block_db.iter().copied().filter(|d| *d > -60.0).collect();
    if loud.len() >= 100 {
        let mean = loud.iter().map(|d| f64::from(*d)).sum::<f64>() / loud.len() as f64;
        let std = (loud
            .iter()
            .map(|d| (f64::from(*d) - mean).powi(2))
            .sum::<f64>()
            / loud.len() as f64)
            .sqrt();
        if a.zero_crossing_rate >= NOISE_ZCR && std < 1.5 {
            out.push(format!(
                "steady broadband noise (zero-crossing rate {:.2}, level variation {std:.1} dB)",
                a.zero_crossing_rate
            ));
        } else if std >= 3.0
            && let Some(period) = loop_period(&a.block_db)
        {
            out.push(format!(
                "the track appears to repeat a {:.1}s segment throughout",
                period as f64 * block_secs
            ));
        }
    }
    out
}

/// Smallest lag (1-15 s) at which the block-energy envelope repeats almost
/// exactly over the whole track: a copy-pasted loop. Needs at least four
/// repetitions and real dynamics (a steady tone trivially "repeats").
fn loop_period(db: &[f32]) -> Option<usize> {
    let min_lag = (1.0 / BLOCK_SECS) as usize;
    let max_lag = (15.0 / BLOCK_SECS) as usize;
    (min_lag..=max_lag)
        .filter(|lag| db.len() >= lag * 4)
        .find(|&lag| {
            let n = db.len() - lag;
            let diff = (0..n)
                .map(|i| f64::from((db[i] - db[i + lag]).abs()))
                .sum::<f64>()
                / n as f64;
            diff < 0.75
        })
}

fn db(linear: f64) -> f64 {
    if linear <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// Sample-format policy (see docs/AUDIO_QC_POLICY.md): masters are delivered
/// byte-for-byte, so only formats every supported DSP ingests unchanged are
/// accepted — integer PCM WAV or FLAC at 16 or 24 bit. 32-bit float and
/// 32-bit integer are refused with an actionable message instead of being
/// silently passed through.
fn sample_format_policy(container: &str, m: &AudioMetrics) -> (bool, String) {
    let codec = m.codec_name.as_str();
    if m.sample_rate > MAX_SAMPLE_RATE {
        return (
            false,
            format!(
                "sample_rate={}: above {MAX_SAMPLE_RATE} Hz is not accepted by DSPs; export at 44.1-192 kHz",
                m.sample_rate
            ),
        );
    }
    let ok = match container {
        "WAV" => {
            m.format_name == "wav"
                && matches!(codec, "pcm_s16le" | "pcm_s24le" | "pcm_u8" | "pcm_s8")
        }
        "FLAC" => {
            m.format_name == "flac" && codec == "flac" && m.bits_per_sample.is_none_or(|b| b <= 24)
        }
        _ => false,
    };
    if ok {
        return (true, format!("codec={codec} bits={:?}", m.bits_per_sample));
    }
    let why = match codec {
        "pcm_f32le" | "pcm_f64le" | "pcm_f32be" | "pcm_f64be" => {
            "32/64-bit floating-point audio is not accepted; export as 16- or 24-bit integer PCM"
        }
        "pcm_s32le" | "pcm_s32be" => {
            "32-bit integer PCM is not accepted; export as 16- or 24-bit PCM"
        }
        "flac" => "FLAC above 24 bit is not accepted; export as 16- or 24-bit FLAC",
        _ => {
            "compressed or unusual audio encoding inside the container; export as 16- or 24-bit PCM WAV or FLAC"
        }
    };
    (
        false,
        format!("codec={codec} format={}: {why}", m.format_name),
    )
}

/// For a RIFF/WAVE file whose `data` chunk declares more bytes than the file
/// contains, return `(declared, available)`. `None` means not truncated (or
/// not determinable: streamed WAVs with a 0/0xFFFFFFFF size, RF64).
pub fn wav_data_truncation(path: &Path) -> Option<(u64, u64)> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr).ok()?;
    if &hdr[0..4] != b"RIFF" || &hdr[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12u64;
    // Bounded walk: a hostile file cannot make this loop forever.
    for _ in 0..4096 {
        if pos + 8 > len {
            return None;
        }
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch).ok()?;
        let size = u64::from(u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]));
        let body = pos + 8;
        if &ch[0..4] == b"data" {
            if size == 0 || size == 0xFFFF_FFFF {
                return None;
            }
            let available = len - body;
            // Allow a partial final sample frame of slack (<= 16 bytes).
            return (available + 16 < size).then_some((size, available));
        }
        pos = body + size + (size & 1);
    }
    None
}

/// Whole-file measurements from one streaming decode pass.
#[derive(Debug, Clone, Default)]
pub struct DecodeAnalysis {
    pub sample_rate: u32,
    pub samples_per_channel: u64,
    /// Absolute sample peak (linear, 0..1).
    pub peak: f32,
    /// Samples that belong to runs of >= CLIP_RUN full-scale samples.
    pub clipped_samples: u64,
    pub clip_events: u64,
    /// EBU R128 integrated loudness; `None` for -inf (silence).
    pub integrated_lufs: Option<f64>,
    /// EBU R128 true peak (dBTP); `None` for -inf (silence).
    pub true_peak_dbtp: Option<f64>,
    /// Absolute sample peak per channel.
    pub channel_peaks: Vec<f32>,
    /// Number of 50 ms blocks, and how many of them are silent (< -60 dBFS).
    pub blocks: u64,
    pub silent_blocks: u64,
    /// Longest run of consecutive silent blocks.
    pub longest_silent_run: u64,
    /// Zero crossings (all channels) over total samples.
    pub zero_crossing_rate: f64,
    /// Per-block RMS level in dB (capped to MAX_ENERGY_BLOCKS), for the
    /// loop / steady-noise heuristics.
    pub block_db: Vec<f32>,
}

/// Block length for silence / structure measurements.
pub const BLOCK_SECS: f64 = 0.05;
/// A block whose peak stays below this (-60 dBFS) counts as silent.
pub const BLOCK_SILENCE_PEAK: f32 = 0.001;
/// Block energies kept for the loop heuristic (50 ms blocks: 30 min).
const MAX_ENERGY_BLOCKS: usize = 36_000;

impl DecodeAnalysis {
    pub fn decoded_secs(&self) -> f64 {
        self.samples_per_channel as f64 / f64::from(self.sample_rate.max(1))
    }
}

/// Time budget for one full decode: generous for long masters (a 45-minute
/// file decodes in well under a minute), and never below the probe timeout.
fn decode_timeout(duration_secs: f64) -> Duration {
    let scaled = Duration::from_secs_f64((60.0 + duration_secs.max(0.0) / 2.0).min(3600.0));
    scaled.max(probe_timeout())
}

/// Decode the file once with ffmpeg: interleaved f32 PCM on stdout feeds the
/// peak / clipping / silence / length measurements (streamed, constant
/// memory), while the ebur128 filter prints loudness and true peak on stderr.
pub fn decode_analysis(
    path: &Path,
    m: &AudioMetrics,
) -> std::result::Result<(DecodeAnalysis, Vec<f32>), AnalyzerError> {
    use AnalyzerError::{Unavailable, Undecodable};
    let channels = m.channels.max(1) as usize;
    let mut child = std::process::Command::new(ffmpeg_bin())
        .args(["-nostdin", "-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-af",
            // framelog=quiet: only the final summary prints.
            "ebur128=peak=true:framelog=quiet",
            "-ac",
            &channels.to_string(),
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Unavailable)?;
    let mut stdout = child.stdout.take().ok_or(Unavailable)?;
    let stderr = child.stderr.take().ok_or(Unavailable)?;
    // stderr: keep only the tail. A damaged file can make ffmpeg log an error
    // per frame; the summary we need is always at the end.
    let err_thread = std::thread::spawn(move || {
        let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
        let mut r = stderr;
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    tail.extend(&buf[..n]);
                    while tail.len() > 65_536 {
                        tail.pop_front();
                    }
                }
            }
        }
        tail.into_iter().collect::<Vec<u8>>()
    });
    let (tx, rx) = std::sync::mpsc::channel();
    let block_frames = ((f64::from(m.sample_rate.max(1)) * BLOCK_SECS) as u64).max(1);
    // Fingerprint tap: the perceptual fingerprint needs mono 11025 Hz PCM
    // over the configured segment windows. Tapping it out of this same
    // decode pass removes the second full ffmpeg decode the fingerprint
    // step used to run (~2.5 s per 3:30 track). Box-filter decimation is a
    // crude anti-alias filter, but the fingerprint only needs log band
    // energies; it is robust to mild aliasing by construction.
    let fp_tap = FpTap::new(
        m.sample_rate.max(1),
        channels,
        crate::fingerprint::segment_windows(m.duration_secs),
    );
    std::thread::spawn(move || {
        let mut tap = fp_tap;
        let mut a = DecodeAnalysis {
            channel_peaks: vec![0.0; channels],
            ..DecodeAnalysis::default()
        };
        let mut st = Meter {
            runs: vec![0u32; channels],
            last_sign: vec![0i8; channels],
            ch: 0,
            samples: 0,
            channels,
            block_frames,
            block_frame: 0,
            block_peak: 0.0,
            block_sq: 0.0,
            silent_run: 0,
            crossings: 0,
        };
        let mut buf = vec![0u8; 1 << 16];
        let mut carry: Vec<u8> = Vec::with_capacity(4);
        loop {
            let n = match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => {
                    let _ = tx.send(None);
                    return;
                }
            };
            let mut data = &buf[..n];
            if !carry.is_empty() {
                let need = 4 - carry.len();
                let take = need.min(data.len());
                carry.extend_from_slice(&data[..take]);
                data = &data[take..];
                if carry.len() < 4 {
                    continue;
                }
                let v = f32::from_le_bytes([carry[0], carry[1], carry[2], carry[3]]);
                carry.clear();
                measure(v, &mut a, &mut st);
                tap.push(v);
            }
            let (chunks, rest) = data.as_chunks::<4>();
            for c in chunks {
                let v = f32::from_le_bytes(*c);
                measure(v, &mut a, &mut st);
                tap.push(v);
            }
            carry.extend_from_slice(rest);
        }
        if st.block_frame > 0 {
            close_block(&mut a, &mut st);
        }
        a.samples_per_channel = st.samples / channels as u64;
        a.zero_crossing_rate = st.crossings as f64 / st.samples.max(1) as f64;
        let _ = tx.send(Some((a, tap.finish())));
    });
    /// Downmix + decimate tap feeding the perceptual fingerprint from the
    /// single decode pass. Only samples inside the configured segment
    /// windows are retained (~4 MB for 3 x 30 s at 11025 Hz mono).
    struct FpTap {
        channels: usize,
        ch: usize,
        frame_sum: f64,
        frame_idx: u64,
        /// Input frames per output sample.
        step: f64,
        bin: u64,
        bin_sum: f64,
        bin_n: u32,
        /// Segment windows in output-sample units: (first_bin, one_past_last).
        windows: Vec<(u64, u64)>,
        samples: Vec<f32>,
    }
    impl FpTap {
        fn new(input_rate: u32, channels: usize, windows_secs: Vec<(f64, f64)>) -> Self {
            let out_rate = crate::fingerprint::FINGERPRINT_SAMPLE_RATE as f64;
            let windows = windows_secs
                .into_iter()
                .map(|(s, len)| {
                    let first = (s.max(0.0) * out_rate) as u64;
                    let last = ((s + len).max(0.0) * out_rate) as u64;
                    (first, last.max(first))
                })
                .collect();
            FpTap {
                channels: channels.max(1),
                ch: 0,
                frame_sum: 0.0,
                frame_idx: 0,
                step: f64::from(input_rate.max(1)) / out_rate,
                bin: 0,
                bin_sum: 0.0,
                bin_n: 0,
                windows,
                samples: Vec::new(),
            }
        }
        fn push(&mut self, v: f32) {
            self.frame_sum += f64::from(if v.is_finite() { v } else { 0.0 });
            self.ch += 1;
            if self.ch < self.channels {
                return;
            }
            self.ch = 0;
            let mono = self.frame_sum / self.channels as f64;
            self.frame_sum = 0.0;
            let o = (self.frame_idx as f64 / self.step) as u64;
            self.frame_idx += 1;
            if o != self.bin {
                self.flush_bin();
                self.bin = o;
            }
            self.bin_sum += mono;
            self.bin_n += 1;
        }
        fn flush_bin(&mut self) {
            if self.bin_n == 0 {
                return;
            }
            let avg = (self.bin_sum / f64::from(self.bin_n)) as f32;
            self.bin_sum = 0.0;
            self.bin_n = 0;
            if self
                .windows
                .iter()
                .any(|(first, last)| self.bin >= *first && self.bin < *last)
            {
                self.samples.push(avg);
            }
        }
        fn finish(mut self) -> Vec<f32> {
            self.flush_bin();
            self.samples
        }
    }
    struct Meter {
        runs: Vec<u32>,
        last_sign: Vec<i8>,
        ch: usize,
        samples: u64,
        channels: usize,
        block_frames: u64,
        block_frame: u64,
        block_peak: f32,
        block_sq: f64,
        silent_run: u64,
        crossings: u64,
    }
    fn close_block(a: &mut DecodeAnalysis, st: &mut Meter) {
        a.blocks += 1;
        if st.block_peak < BLOCK_SILENCE_PEAK {
            a.silent_blocks += 1;
            st.silent_run += 1;
            a.longest_silent_run = a.longest_silent_run.max(st.silent_run);
        } else {
            st.silent_run = 0;
        }
        if a.block_db.len() < MAX_ENERGY_BLOCKS {
            let n = (st.block_frame * st.channels as u64).max(1) as f64;
            let rms = (st.block_sq / n).sqrt();
            a.block_db.push((20.0 * rms.max(1e-9).log10()) as f32);
        }
        st.block_frame = 0;
        st.block_peak = 0.0;
        st.block_sq = 0.0;
    }
    fn measure(v: f32, a: &mut DecodeAnalysis, st: &mut Meter) {
        let v = if v.is_finite() { v } else { 0.0 };
        let x = v.abs();
        if x > a.peak {
            a.peak = x;
        }
        let ch = st.ch;
        if x > a.channel_peaks[ch] {
            a.channel_peaks[ch] = x;
        }
        let sign = if v > 1e-6 {
            1
        } else if v < -1e-6 {
            -1
        } else {
            0
        };
        if sign != 0 {
            if st.last_sign[ch] != 0 && sign != st.last_sign[ch] {
                st.crossings += 1;
            }
            st.last_sign[ch] = sign;
        }
        if x > st.block_peak {
            st.block_peak = x;
        }
        st.block_sq += f64::from(v) * f64::from(v);
        let run = &mut st.runs[ch];
        if x >= CLIP_LEVEL {
            *run += 1;
            if *run == CLIP_RUN {
                a.clip_events += 1;
                a.clipped_samples += u64::from(CLIP_RUN);
            } else if *run > CLIP_RUN {
                a.clipped_samples += 1;
            }
        } else {
            *run = 0;
        }
        st.ch = (ch + 1) % st.channels;
        st.samples += 1;
        if st.ch == 0 {
            st.block_frame += 1;
            if st.block_frame == st.block_frames {
                close_block(a, st);
            }
        }
    }
    let (analysis, fp_samples) = match rx.recv_timeout(decode_timeout(m.duration_secs)) {
        Ok(Some((a, s))) => (a, s),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Unavailable);
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Unavailable);
        }
    };
    let status = child.wait().map_err(|_| Unavailable)?;
    let err_text = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(Undecodable);
    }
    let (integrated, true_peak) =
        parse_ebur128(&String::from_utf8_lossy(&err_text)).ok_or(Undecodable)?;
    Ok((
        DecodeAnalysis {
            sample_rate: m.sample_rate,
            integrated_lufs: integrated,
            true_peak_dbtp: true_peak,
            ..analysis
        },
        fp_samples,
    ))
}

fn ffmpeg_bin() -> String {
    std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into())
}

/// Spotify normalization target: -14 LUFS. ±1 LU is the practical tolerance
/// band used across distributors; misses are review-only warnings.
pub const LOUDNESS_TARGET_LUFS: f64 = -14.0;
pub const LOUDNESS_TOLERANCE_LU: f64 = 1.0;
/// True peak ceiling: -1 dBTP (Spotify asks -2 dB when the master is hot).
pub const TRUE_PEAK_MAX_DBTP: f64 = -1.0;

/// Parse the ebur128 summary. Each value is `Some(finite)` or `None` for
/// `-inf` (digital silence); a missing summary is a parse failure.
fn parse_ebur128(text: &str) -> Option<(Option<f64>, Option<f64>)> {
    let mut integrated = None;
    let mut peak = None;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        match line.trim() {
            "Integrated loudness:" => integrated = lines.next().and_then(parse_ebur128_value),
            "True peak:" => peak = lines.next().and_then(parse_ebur128_value),
            _ => {}
        }
    }
    Some((integrated?, peak?))
}

fn parse_ebur128_value(line: &str) -> Option<Option<f64>> {
    // "    I:         -13.8 LUFS" / "    Peak:      -10.1 dBFS" / "    I:          -inf LUFS"
    let num = line.split(':').nth(1)?.split_whitespace().next()?;
    if num.eq_ignore_ascii_case("-inf") {
        return Some(None);
    }
    num.parse::<f64>().ok().filter(|f| f.is_finite()).map(Some)
}

/// Fixed Stage 1 image check contract: every analyzed cover-art asset yields
/// exactly these five outcomes, in order.
pub const IMAGE_CHECK_CODES: &[&str] = &[
    "SHA256_MISMATCH",
    "IMAGE_MAGIC_MISMATCH",
    "IMAGE_PROBE_FAILED",
    "IMAGE_TOO_SMALL",
    "IMAGE_NOT_SQUARE",
];

/// Stage 1 basic QC for a cover-art image file.
pub fn check_image(path: &Path, registered_sha256: Option<&str>) -> Vec<CheckOutcome> {
    /// Emit `IMAGE_CHECK_CODES[from..]` with a uniform status (short-circuit tail).
    fn tail(from: usize, status: CheckStatus, input_hash: &str, detail: &str) -> Vec<CheckOutcome> {
        IMAGE_CHECK_CODES[from..]
            .iter()
            .map(|code| CheckOutcome {
                check_code: code,
                status,
                input_hash: input_hash.to_string(),
                detail: detail.into(),
            })
            .collect()
    }
    let actual = match sha256_file(path) {
        Ok(h) => h,
        Err(_) => {
            return tail(
                0,
                CheckStatus::TechnicalRetry,
                &metric_hash(&["unreadable"]),
                "cannot read file",
            );
        }
    };
    let mut out = Vec::with_capacity(IMAGE_CHECK_CODES.len());
    let sha_status = match registered_sha256 {
        Some(expected) if expected == actual => CheckStatus::Pass,
        Some(_) => CheckStatus::Blocked,
        None => CheckStatus::Pass,
    };
    out.push(CheckOutcome {
        check_code: "SHA256_MISMATCH",
        status: sha_status,
        input_hash: metric_hash(&[&actual]),
        detail: match registered_sha256 {
            Some(expected) => format!("registered={expected} actual={actual}"),
            None => "no registered hash".into(),
        },
    });
    if sha_status == CheckStatus::Blocked {
        out.extend(tail(
            1,
            CheckStatus::NotApplicable,
            &metric_hash(&[&actual]),
            "sha256 mismatch",
        ));
        return out;
    }
    let head = head_bytes(path).unwrap_or_default();
    let container = detect_container(&head);
    let magic_ok = matches!(container, "PNG" | "JPEG");
    out.push(CheckOutcome {
        check_code: "IMAGE_MAGIC_MISMATCH",
        status: if magic_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        input_hash: metric_hash(&[container]),
        detail: format!("detected={container}"),
    });
    if !magic_ok {
        out.extend(tail(
            2,
            CheckStatus::NotApplicable,
            &metric_hash(&[container]),
            "not an image container",
        ));
        return out;
    }
    let metrics = match probe_classified(path)
        .and_then(|v| parse_image(&v).map_err(|_| AnalyzerError::Undecodable))
    {
        Ok(m) => m,
        Err(e) => {
            // A corrupt image is a deterministic, user-fixable defect; only an
            // unavailable analyzer is worth a retry.
            let (status, detail, rest) = match e {
                AnalyzerError::Undecodable => (
                    CheckStatus::CorrectionRequired,
                    "the image cannot be decoded (corrupted file); export it again as JPEG or PNG",
                    CheckStatus::NotApplicable,
                ),
                AnalyzerError::Unavailable => (
                    CheckStatus::TechnicalRetry,
                    "ffprobe unavailable or timed out",
                    CheckStatus::TechnicalRetry,
                ),
            };
            out.push(CheckOutcome {
                check_code: "IMAGE_PROBE_FAILED",
                status,
                input_hash: metric_hash(&[&actual]),
                detail: detail.into(),
            });
            out.extend(tail(3, rest, &metric_hash(&[&actual]), "probe failed"));
            return out;
        }
    };
    out.push(CheckOutcome {
        check_code: "IMAGE_PROBE_FAILED",
        status: CheckStatus::Pass,
        input_hash: metric_hash(&[&actual]),
        detail: "ffprobe ok".into(),
    });
    let long_side = metrics.width.max(metrics.height);
    out.push(CheckOutcome {
        check_code: "IMAGE_TOO_SMALL",
        status: if long_side >= MIN_IMAGE_LONG_SIDE {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        input_hash: metric_hash(&[&metrics.width.to_string(), &metrics.height.to_string()]),
        detail: format!("{}x{}", metrics.width, metrics.height),
    });
    // Every DSP requires 1:1 cover art. Non-square is an objective,
    // user-fixable defect, so it blocks like a too-small image.
    out.push(CheckOutcome {
        check_code: "IMAGE_NOT_SQUARE",
        status: if metrics.width == metrics.height {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        input_hash: metric_hash(&[&metrics.width.to_string(), &metrics.height.to_string()]),
        detail: format!("{}x{}", metrics.width, metrics.height),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("audeniq-qc-test-{name}"));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn make_wav(path: &Path, secs: u32, rate: u32) {
        let st = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("sine=frequency=440:duration={secs}:sample_rate={rate}"),
                "-c:a",
                "pcm_s16le",
                &path.to_string_lossy(),
            ])
            .status()
            .expect("ffmpeg missing");
        assert!(st.success());
    }

    fn make_png(path: &Path, w: u32, h: u32) {
        let st = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=red:size={w}x{h}:duration=1"),
                "-frames:v",
                "1",
                &path.to_string_lossy(),
            ])
            .status()
            .expect("ffmpeg missing");
        assert!(st.success());
    }

    #[test]
    fn detect_container_tags() {
        assert_eq!(detect_container(b"RIFF\x00\x00\x00\x00WAVE"), "WAV");
        assert_eq!(detect_container(b"RF64\xff\xff\xff\xffWAVE"), "WAV");
        assert_eq!(detect_container(b"RIFX\x00\x00\x00\x00WAVE"), "UNKNOWN");
        assert_eq!(detect_container(b"fLaC\x00"), "FLAC");
        assert_eq!(detect_container(b"ID3\x04\x00"), "MP3");
        assert_eq!(detect_container(b"\x89PNG\r\n\x1a\n"), "PNG");
        assert_eq!(detect_container(b"\xFF\xD8\xFF\xE0"), "JPEG");
        assert_eq!(detect_container(b"garbage-data-here"), "UNKNOWN");
    }

    #[test]
    fn audio_passes_on_clean_wav() {
        // The default sine sits at about -21.8 LUFS (review-only loudness
        // flag), so the all-pass fixture is mastered into the -14 ±1 band.
        let p = tmp("clean.wav");
        let st = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=60:sample_rate=44100",
                "-filter:a",
                "volume=8dB",
                "-c:a",
                "pcm_s16le",
                &p.to_string_lossy(),
            ])
            .status()
            .expect("ffmpeg missing");
        assert!(st.success());
        let out = check_audio(&p, None, Some("audio/wav"));
        assert!(out.iter().all(|o| o.status == CheckStatus::Pass), "{out:?}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_flags_short_and_low_rate() {
        let p = tmp("short.wav");
        make_wav(&p, 5, 22050);
        let out = check_audio(&p, None, Some("audio/wav"));
        let short = out
            .iter()
            .find(|o| o.check_code == "AUDIO_TOO_SHORT")
            .unwrap();
        assert_eq!(short.status, CheckStatus::CorrectionRequired);
        let rate = out
            .iter()
            .find(|o| o.check_code == "AUDIO_SAMPLE_RATE_LOW")
            .unwrap();
        assert_eq!(rate.status, CheckStatus::CorrectionRequired);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_rejects_garbage_bytes() {
        let p = tmp("garbage.wav");
        std::fs::write(&p, b"this is not audio at all, just text").unwrap();
        let out = check_audio(&p, None, Some("audio/wav"));
        let magic = out
            .iter()
            .find(|o| o.check_code == "AUDIO_MAGIC_MISMATCH")
            .unwrap();
        assert_eq!(magic.status, CheckStatus::CorrectionRequired);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_blocks_sha_mismatch() {
        let p = tmp("mismatch.wav");
        make_wav(&p, 60, 44100);
        let out = check_audio(&p, Some(&"0".repeat(64)), Some("audio/wav"));
        let sha = out
            .iter()
            .find(|o| o.check_code == "SHA256_MISMATCH")
            .unwrap();
        assert_eq!(sha.status, CheckStatus::Blocked);
        // Blocked short-circuits: the remaining checks are NOT_APPLICABLE,
        // but the full contract still holds (13 QC + 2 fingerprint).
        assert_eq!(out.len(), 15);
        assert!(
            out[1..]
                .iter()
                .all(|o| o.status == CheckStatus::NotApplicable)
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn image_flags_small_cover() {
        let p = tmp("small.png");
        make_png(&p, 500, 500);
        let out = check_image(&p, None);
        let small = out
            .iter()
            .find(|o| o.check_code == "IMAGE_TOO_SMALL")
            .unwrap();
        assert_eq!(small.status, CheckStatus::CorrectionRequired);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn image_passes_on_large_cover() {
        let p = tmp("large.png");
        make_png(&p, 3000, 3000);
        let out = check_image(&p, None);
        assert!(out.iter().all(|o| o.status == CheckStatus::Pass), "{out:?}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn image_flags_nonsquare_cover() {
        let p = tmp("nonsquare.png");
        make_png(&p, 3000, 2000);
        let out = check_image(&p, None);
        let sq = out
            .iter()
            .find(|o| o.check_code == "IMAGE_NOT_SQUARE")
            .unwrap();
        assert_eq!(sq.status, CheckStatus::CorrectionRequired);
        // 3000px long side still passes the size gate; only shape fails.
        let small = out
            .iter()
            .find(|o| o.check_code == "IMAGE_TOO_SMALL")
            .unwrap();
        assert_eq!(small.status, CheckStatus::Pass);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_flags_loudness_out_of_range() {
        // Default lavfi sine measures about -21.8 LUFS: outside -14 ±1,
        // so the check trips as review-only (DSPs normalize, never reject).
        let p = tmp("quiet.wav");
        make_wav(&p, 60, 44100);
        let out = check_audio(&p, None, Some("audio/wav"));
        let loud = out
            .iter()
            .find(|o| o.check_code == "AUDIO_LOUDNESS_OUT_OF_RANGE")
            .unwrap();
        assert_eq!(loud.status, CheckStatus::ReviewRequired);
        assert!(loud.detail.contains("integrated_lufs="), "{}", loud.detail);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_passes_loudness_in_range() {
        // +8 dB on the same sine lands at about -13.8 LUFS: inside the band.
        let p = tmp("hot.wav");
        let st = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=60:sample_rate=44100",
                "-filter:a",
                "volume=8dB",
                "-c:a",
                "pcm_s16le",
                &p.to_string_lossy(),
            ])
            .status()
            .expect("ffmpeg missing");
        assert!(st.success());
        let out = check_audio(&p, None, Some("audio/wav"));
        let loud = out
            .iter()
            .find(|o| o.check_code == "AUDIO_LOUDNESS_OUT_OF_RANGE")
            .unwrap();
        assert_eq!(loud.status, CheckStatus::Pass, "{}", loud.detail);
        let _ = std::fs::remove_file(&p);
    }

    /// Render `lavfi` source `src` through `extra` ffmpeg output args.
    fn render(path: &Path, src: &str, extra: &[&str]) {
        let st = Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", src])
            .args(extra)
            .arg(path)
            .status()
            .expect("ffmpeg missing");
        assert!(st.success());
    }

    fn status_of(out: &[CheckOutcome], code: &str) -> CheckStatus {
        out.iter()
            .find(|o| o.check_code == code)
            .unwrap_or_else(|| panic!("{code} missing: {out:?}"))
            .status
    }

    const HOT_SINE: &str = "sine=frequency=440:duration=40:sample_rate=48000";

    #[test]
    fn audio_rejects_mp3_declared_as_wav() {
        // Sandbox e02: an MP3 renamed .wav used to be distributed as WAV.
        let p = tmp("renamed-mp3.wav");
        render(
            &p,
            HOT_SINE,
            &["-c:a", "libmp3lame", "-b:a", "192k", "-f", "mp3"],
        );
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_MAGIC_MISMATCH"),
            CheckStatus::CorrectionRequired
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_rejects_flac_declared_as_wav_but_accepts_declared_flac() {
        let p = tmp("renamed-flac.wav");
        render(
            &p,
            HOT_SINE,
            &[
                "-af",
                "volume=8dB",
                "-c:a",
                "flac",
                "-sample_fmt",
                "s16",
                "-f",
                "flac",
            ],
        );
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_MAGIC_MISMATCH"),
            CheckStatus::CorrectionRequired
        );
        // The same bytes declared as FLAC are a valid 16-bit master: bit depth
        // comes from bits_per_raw_sample, not the (absent) bits_per_sample.
        let out = check_audio(&p, None, Some("audio/flac"));
        assert!(out.iter().all(|o| o.status == CheckStatus::Pass), "{out:?}");
        let _ = std::fs::remove_file(&p);
    }

    fn suspect(out: &[CheckOutcome]) -> &CheckOutcome {
        out.iter()
            .find(|o| o.check_code == "AUDIO_CONTENT_SUSPECT")
            .expect("AUDIO_CONTENT_SUSPECT missing")
    }

    #[test]
    fn content_suspect_flags_sandbox_round2_patterns() {
        // Sandbox round 2: each of these reached a DSP without any review.
        let cases: &[(&str, &str, &[&str], &str)] = &[
            (
                "noise",
                "anoisesrc=color=white:amplitude=0.5:duration=40:sample_rate=48000",
                &["-c:a", "pcm_s16le"],
                "noise",
            ),
            (
                "deadch",
                "sine=frequency=440:duration=40:sample_rate=48000",
                &[
                    "-af",
                    "volume=8dB,pan=stereo|c0=c0|c1=0*c0",
                    "-c:a",
                    "pcm_s16le",
                ],
                "one channel is silent",
            ),
            (
                "nearsilent",
                "sine=frequency=440:duration=40:sample_rate=48000",
                &["-af", "volume=-45dB", "-c:a", "pcm_s16le"],
                "near-silent",
            ),
            (
                "padded",
                "sine=frequency=440:duration=10:sample_rate=48000",
                &["-af", "volume=8dB,apad=whole_dur=50", "-c:a", "pcm_s16le"],
                "long silence",
            ),
            (
                "loop",
                "aevalsrc='0.5*sin(2*PI*440*t)*(0.1+0.9*mod(t\\,5)/5)':d=60:s=48000",
                &["-c:a", "pcm_s16le"],
                "repeat",
            ),
        ];
        for (name, src, extra, want) in cases {
            let p = tmp(&format!("suspect-{name}.wav"));
            render(&p, src, extra);
            let out = check_audio(&p, None, Some("audio/wav"));
            let s = suspect(&out);
            assert_eq!(
                s.status,
                CheckStatus::ReviewRequired,
                "{name}: {}",
                s.detail
            );
            assert!(s.detail.contains(want), "{name}: {}", s.detail);
            // Review only: nothing here may become a blocking correction.
            assert!(
                out.iter()
                    .all(|o| o.status != CheckStatus::CorrectionRequired),
                "{name}: {out:?}"
            );
            let _ = std::fs::remove_file(&p);
        }
    }

    #[test]
    fn content_suspect_passes_ordinary_programme() {
        // A non-repeating, dynamic stereo signal must not be flagged.
        let p = tmp("suspect-ordinary.wav");
        render(
            &p,
            "aevalsrc='0.4*sin(2*PI*(220+40*sin(0.31*t))*t)*(0.55+0.45*sin(0.17*t*t))|0.4*sin(2*PI*(330+30*sin(0.23*t))*t)*(0.55+0.45*cos(0.13*t*t))':d=60:s=48000",
            &["-c:a", "pcm_s16le"],
        );
        let out = check_audio(&p, None, Some("audio/wav"));
        let s = suspect(&out);
        assert_eq!(s.status, CheckStatus::Pass, "{}", s.detail);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_rejects_sample_rate_above_192k_without_decoding() {
        let p = tmp("384k.wav");
        render(
            &p,
            "sine=frequency=440:duration=5:sample_rate=384000",
            &["-c:a", "pcm_s16le"],
        );
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_SAMPLE_FORMAT_UNSUPPORTED"),
            CheckStatus::CorrectionRequired
        );
        assert_eq!(
            status_of(&out, "AUDIO_TRUNCATED"),
            CheckStatus::NotApplicable
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_rejects_float_wav() {
        let p = tmp("float.wav");
        render(&p, HOT_SINE, &["-c:a", "pcm_f32le"]);
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_SAMPLE_FORMAT_UNSUPPORTED"),
            CheckStatus::CorrectionRequired
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_rejects_truncated_wav() {
        // Sandbox e05: a WAV cut at 80% passed and was distributed.
        let p = tmp("truncated.wav");
        render(&p, HOT_SINE, &["-af", "volume=8dB", "-c:a", "pcm_s16le"]);
        let len = std::fs::metadata(&p).unwrap().len();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_len(len * 8 / 10).unwrap();
        let out = check_audio(&p, None, Some("audio/wav"));
        let t = out
            .iter()
            .find(|o| o.check_code == "AUDIO_TRUNCATED")
            .unwrap();
        assert_eq!(t.status, CheckStatus::CorrectionRequired, "{}", t.detail);
        assert!(t.detail.contains("truncated"), "{}", t.detail);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_silence_is_rejected_without_parse_failure() {
        // Sandbox e08: loudness `-inf` used to fail parsing and retry forever.
        let p = tmp("silent.wav");
        render(
            &p,
            "anullsrc=r=48000:cl=stereo",
            &["-t", "40", "-c:a", "pcm_s16le"],
        );
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_SILENT"),
            CheckStatus::CorrectionRequired
        );
        assert!(
            out.iter().all(|o| o.status != CheckStatus::TechnicalRetry),
            "{out:?}"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_rejects_hard_clipping_and_warns_on_hot_peaks() {
        let p = tmp("clipped.wav");
        render(&p, HOT_SINE, &["-af", "volume=30dB", "-c:a", "pcm_s16le"]);
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_CLIPPING"),
            CheckStatus::CorrectionRequired
        );
        // Loudness is never a hard gate, however hot the master is.
        assert_ne!(
            status_of(&out, "AUDIO_LOUDNESS_OUT_OF_RANGE"),
            CheckStatus::CorrectionRequired
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_corrupt_header_is_correction_not_retry() {
        // Sandbox e07: valid RIFF signature, garbage after it.
        let p = tmp("corrupt-header.wav");
        let mut bytes = b"RIFF\x24\x00\x10\x00WAVEjunk".to_vec();
        bytes.extend((0..200_000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8));
        std::fs::write(&p, &bytes).unwrap();
        let out = check_audio(&p, None, Some("audio/wav"));
        assert_eq!(
            status_of(&out, "AUDIO_PROBE_FAILED"),
            CheckStatus::CorrectionRequired
        );
        assert!(
            out.iter().all(|o| o.status != CheckStatus::TechnicalRetry),
            "{out:?}"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ebur128_parse_reads_summary() {
        let text = "  Integrated loudness:\n    I:         -13.8 LUFS\n  True peak:\n    Peak:      -10.1 dBFS\n";
        let (i, p) = parse_ebur128(text).unwrap();
        assert!((i.unwrap() - -13.8).abs() < 0.01);
        assert!((p.unwrap() - -10.1).abs() < 0.01);
        // Digital silence: ffmpeg prints -inf / -70 floor; must not fail parsing.
        let silent = "  Integrated loudness:\n    I:         -inf LUFS\n  True peak:\n    Peak:      -inf dBFS\n";
        let (i, p) = parse_ebur128(silent).unwrap();
        assert!(i.is_none() && p.is_none());
        assert!(parse_ebur128("garbage").is_none());
    }

    #[test]
    fn result_hash_is_stable() {
        let a = result_hash("AUDIO_TOO_SHORT", "1", &"a".repeat(64), "metrics");
        let b = result_hash("AUDIO_TOO_SHORT", "1", &"a".repeat(64), "metrics");
        let c = result_hash("AUDIO_TOO_SHORT", "2", &"a".repeat(64), "metrics");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn probe_kills_hanging_analyzer() {
        // A fake "ffprobe" that never exits: probe_with must give up after the
        // timeout instead of wedging the worker. No env mutation (parallel-safe).
        let script = tmp("hang.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let target = tmp("target.wav");
        std::fs::write(&target, b"RIFF....").unwrap();
        let start = std::time::Instant::now();
        let r = probe_with(&script.to_string_lossy(), &target, Duration::from_secs(2));
        assert!(r.is_err(), "hung analyzer must fail, not hang");
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "probe returned only after {:?}",
            start.elapsed()
        );
        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&target);
    }
}
