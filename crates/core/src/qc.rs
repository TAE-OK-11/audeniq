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
pub const QC_RULE_VERSION: &str = "1";

/// Minimum audio duration in seconds before flagging as suspiciously short.
pub const MIN_AUDIO_SECS: f64 = 30.0;
/// Minimum sample rate accepted without correction.
pub const MIN_SAMPLE_RATE: u32 = 44_100;
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
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u32,
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

/// Run ffprobe and parse its JSON report. Any failure (missing binary,
/// undecodable file, timeout, oversized output) is an error; callers map it to
/// TECHNICAL_RETRY.
pub fn probe(path: &Path) -> Result<serde_json::Value> {
    probe_with(&ffprobe_bin(), path, probe_timeout())
}

fn probe_with(bin: &str, path: &Path, timeout: Duration) -> Result<serde_json::Value> {
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
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Internal)?;
    let stdout = child.stdout.take().ok_or(Error::Internal)?;
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
            return Err(Error::Internal);
        }
    };
    let status = child.wait().map_err(|_| Error::Internal)?;
    if !status.success() || buf.len() as u64 > PROBE_OUTPUT_MAX {
        return Err(Error::Internal);
    }
    serde_json::from_slice(&buf).map_err(|_| Error::Internal)
}

fn parse_audio(v: &serde_json::Value) -> Result<AudioMetrics> {
    let p: ProbeJson = serde_json::from_value(v.clone()).map_err(|_| Error::Internal)?;
    let s = p
        .streams
        .iter()
        .find(|s| s.codec_type == "audio")
        .ok_or(Error::Internal)?;
    Ok(AudioMetrics {
        format_name: p.format.format_name.split(',').next().unwrap_or("").into(),
        duration_secs: p.format.duration.parse().unwrap_or(0.0),
        sample_rate: s.sample_rate.parse().unwrap_or(0),
        channels: s.channels,
        bits_per_sample: if s.bits_per_sample > 0 {
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

/// Detect container from magic bytes. Returns a canonical tag or "UNKNOWN".
pub fn detect_container(head: &[u8]) -> &'static str {
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
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

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|_| Error::Internal)?;
    let mut h = Sha256::new();
    h.update(&bytes);
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
/// these six outcomes, in order. Later stages and the Studio UI can rely on the
/// set being stable; checks that could not run are NOT_APPLICABLE or
/// TECHNICAL_RETRY instead of being silently dropped.
pub const AUDIO_CHECK_CODES: &[&str] = &[
    "SHA256_MISMATCH",
    "AUDIO_MAGIC_MISMATCH",
    "AUDIO_PROBE_FAILED",
    "AUDIO_TOO_SHORT",
    "AUDIO_SAMPLE_RATE_LOW",
    "AUDIO_BIT_DEPTH_LOW",
    "AUDIO_CHANNEL_INVALID",
];

/// Stage 1 basic QC for an audio asset file.
pub fn check_audio(path: &Path, registered_sha256: Option<&str>) -> Vec<CheckOutcome> {
    /// Emit `AUDIO_CHECK_CODES[from..]` with a uniform status (short-circuit tail).
    fn tail(from: usize, status: CheckStatus, input_hash: &str, detail: &str) -> Vec<CheckOutcome> {
        AUDIO_CHECK_CODES[from..]
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
            1,
            CheckStatus::NotApplicable,
            &metric_hash(&[&actual]),
            "sha256 mismatch",
        ));
        return out;
    }
    let head = head_bytes(path).unwrap_or_default();
    let container = detect_container(&head);
    let magic_ok = matches!(container, "WAV" | "FLAC" | "MP3");
    out.push(CheckOutcome {
        check_code: "AUDIO_MAGIC_MISMATCH",
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
            "not an audio container",
        ));
        return out;
    }
    let metrics = match probe(path).ok().and_then(|v| parse_audio(&v).ok()) {
        Some(m) => m,
        None => {
            out.push(CheckOutcome {
                check_code: "AUDIO_PROBE_FAILED",
                status: CheckStatus::TechnicalRetry,
                input_hash: metric_hash(&[&actual]),
                detail: "ffprobe failed".into(),
            });
            // Transient: metrics cannot be assessed yet; the worker retries later.
            out.extend(tail(
                3,
                CheckStatus::TechnicalRetry,
                &metric_hash(&[&actual]),
                "probe failed",
            ));
            return out;
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
        &metrics.duration_secs.to_string(),
        &metrics.sample_rate.to_string(),
        &metrics.channels.to_string(),
    ]);
    out.push(CheckOutcome {
        check_code: "AUDIO_TOO_SHORT",
        status: if metrics.duration_secs >= MIN_AUDIO_SECS {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        input_hash: mh.clone(),
        detail: format!("duration_secs={:.2}", metrics.duration_secs),
    });
    out.push(CheckOutcome {
        check_code: "AUDIO_SAMPLE_RATE_LOW",
        status: if metrics.sample_rate >= MIN_SAMPLE_RATE {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        input_hash: mh.clone(),
        detail: format!("sample_rate={}", metrics.sample_rate),
    });
    // Spotify: below 16-bit is upconverted but ineligible for lossless.
    // Unknown bit depth (probe didn't report it) is not a failure — the
    // checks we can't verify, we don't block on.
    out.push(CheckOutcome {
        check_code: "AUDIO_BIT_DEPTH_LOW",
        status: match metrics.bits_per_sample {
            Some(b) if b < MIN_BIT_DEPTH => CheckStatus::CorrectionRequired,
            _ => CheckStatus::Pass,
        },
        input_hash: mh.clone(),
        detail: format!("bits_per_sample={:?}", metrics.bits_per_sample),
    });
    out.push(CheckOutcome {
        check_code: "AUDIO_CHANNEL_INVALID",
        status: if metrics.channels == 1 || metrics.channels == 2 {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        input_hash: mh,
        detail: format!("channels={}", metrics.channels),
    });
    out
}

/// Fixed Stage 1 image check contract: every analyzed cover-art asset yields
/// exactly these four outcomes, in order.
pub const IMAGE_CHECK_CODES: &[&str] = &[
    "SHA256_MISMATCH",
    "IMAGE_MAGIC_MISMATCH",
    "IMAGE_PROBE_FAILED",
    "IMAGE_TOO_SMALL",
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
    let metrics = match probe(path).ok().and_then(|v| parse_image(&v).ok()) {
        Some(m) => m,
        None => {
            out.push(CheckOutcome {
                check_code: "IMAGE_PROBE_FAILED",
                status: CheckStatus::TechnicalRetry,
                input_hash: metric_hash(&[&actual]),
                detail: "ffprobe failed".into(),
            });
            out.extend(tail(
                3,
                CheckStatus::TechnicalRetry,
                &metric_hash(&[&actual]),
                "probe failed",
            ));
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
        assert_eq!(detect_container(b"fLaC\x00"), "FLAC");
        assert_eq!(detect_container(b"ID3\x04\x00"), "MP3");
        assert_eq!(detect_container(b"\x89PNG\r\n\x1a\n"), "PNG");
        assert_eq!(detect_container(b"\xFF\xD8\xFF\xE0"), "JPEG");
        assert_eq!(detect_container(b"garbage-data-here"), "UNKNOWN");
    }

    #[test]
    fn audio_passes_on_clean_wav() {
        let p = tmp("clean.wav");
        make_wav(&p, 60, 44100);
        let out = check_audio(&p, None);
        assert!(out.iter().all(|o| o.status == CheckStatus::Pass), "{out:?}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn audio_flags_short_and_low_rate() {
        let p = tmp("short.wav");
        make_wav(&p, 5, 22050);
        let out = check_audio(&p, None);
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
        let out = check_audio(&p, None);
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
        let out = check_audio(&p, Some(&"0".repeat(64)));
        let sha = out
            .iter()
            .find(|o| o.check_code == "SHA256_MISMATCH")
            .unwrap();
        assert_eq!(sha.status, CheckStatus::Blocked);
        // Blocked short-circuits: the remaining checks are NOT_APPLICABLE,
        // but the seven-check contract still holds.
        assert_eq!(out.len(), 7);
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
