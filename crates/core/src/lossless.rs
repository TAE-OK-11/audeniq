//! Normalize supported lossless uploads to a DSP-compatible FLAC master.
//! Container labels alone never establish that a codec is lossless.
use crate::qc;
use std::{
    ffi::OsStr,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

type ConversionResult<T> = std::result::Result<T, &'static str>;
const MAX_OUTPUT: u64 = 4096;

fn run(args: &[&OsStr], timeout: Duration) -> ConversionResult<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut command =
        std::process::Command::new(std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into()));
    command.args(args);
    // Only real input files are readable. The one FLAC output is granted
    // separately; no access to neighboring uploads, keyrings or credentials.
    let inputs: Vec<_> = args
        .iter()
        .map(Path::new)
        .filter(|p| p.is_absolute() && p.is_file())
        .collect();
    let outputs: Vec<_> = args
        .windows(3)
        .filter(|a| a[0] == OsStr::new("-f") && a[1] == OsStr::new("flac"))
        .map(|a| Path::new(a[2]))
        .collect();
    crate::parser_sandbox::restrict(&mut command, &inputs, &outputs)
        .map_err(|_| "UPLOAD_CONVERSION_UNAVAILABLE")?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "UPLOAD_CONVERSION_UNAVAILABLE")?;
    let stdout = child.stdout.take().ok_or("UPLOAD_CONVERSION_UNAVAILABLE")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let result = stdout
            .take(MAX_OUTPUT + 1)
            .read_to_end(&mut out)
            .map(|_| out);
        let _ = tx.send(result);
    });
    let result = (|| {
        let out = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "UPLOAD_CONVERSION_TIMEOUT")?
            .map_err(|_| "UPLOAD_CONVERSION_FAILED")?;
        if out.len() as u64 > MAX_OUTPUT {
            return Err("UPLOAD_CONVERSION_FAILED");
        }
        // EOF is not proof that the process exited. Enforce the same deadline
        // even if a broken analyzer closes stdout and then hangs.
        loop {
            if let Some(status) = child.try_wait().map_err(|_| "UPLOAD_CONVERSION_FAILED")? {
                return if status.success() {
                    Ok(out)
                } else {
                    Err("UPLOAD_CONVERSION_FAILED")
                };
            }
            if Instant::now() >= deadline {
                return Err("UPLOAD_CONVERSION_TIMEOUT");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn hash_output(out: &[u8]) -> ConversionResult<String> {
    let text = std::str::from_utf8(out).map_err(|_| "UPLOAD_CONVERSION_FAILED")?;
    let hash = text
        .trim()
        .strip_prefix("SHA256=")
        .ok_or("UPLOAD_CONVERSION_FAILED")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("UPLOAD_CONVERSION_FAILED");
    }
    Ok(hash.to_ascii_lowercase())
}

pub(crate) fn pcm_sha256(path: &Path) -> ConversionResult<String> {
    let args: Vec<&OsStr> = vec![
        "-nostdin".as_ref(),
        "-v".as_ref(),
        "error".as_ref(),
        "-xerror".as_ref(),
        "-protocol_whitelist".as_ref(),
        "file".as_ref(),
        "-threads".as_ref(),
        "1".as_ref(),
        "-i".as_ref(),
        path.as_os_str(),
        "-map".as_ref(),
        "0:a:0".as_ref(),
        "-c:a".as_ref(),
        "pcm_s32le".as_ref(),
        "-f".as_ref(),
        "hash".as_ref(),
        "-hash".as_ref(),
        "sha256".as_ref(),
        "pipe:1".as_ref(),
    ];
    hash_output(&run(&args, Duration::from_secs(600))?)
}

/// Read every WavPack block, not just the first: hybrid mode can change
/// between blocks. Reject hybrid, float and DSD. No correction sidecars.
/// Layout: https://www.wavpack.com/WavPack5FileFormat.pdf
fn validate_wavpack(path: &Path) -> ConversionResult<()> {
    let mut f = std::fs::File::open(path).map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
    let mut end = f.metadata().map_err(|_| "UPLOAD_CONTENT_MISMATCH")?.len();
    let file_end = end;
    // Exclude recognized trailing tags; never scan arbitrary bytes for a
    // block signature (a fake signature inside metadata is not audio).
    if end >= 128 {
        f.seek(SeekFrom::Start(end - 128))
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        let mut tag = [0; 3];
        f.read_exact(&mut tag)
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        if &tag == b"TAG" {
            end -= 128;
        }
    }
    if end >= 32 {
        f.seek(SeekFrom::Start(end - 32))
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        let mut footer = [0; 32];
        f.read_exact(&mut footer)
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        if &footer[..8] == b"APETAGEX" {
            let size = u32::from_le_bytes(footer[12..16].try_into().unwrap()) as u64;
            if size < 32 || size > end {
                return Err("UPLOAD_CONTENT_MISMATCH");
            }
            end -= size;
            if end >= 32 {
                f.seek(SeekFrom::Start(end - 32))
                    .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
                let mut header = [0; 8];
                f.read_exact(&mut header)
                    .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
                if &header == b"APETAGEX" {
                    end -= 32;
                }
            }
        }
    }
    // A forged APE size must not hide later hybrid blocks from our checks
    // while the decoder still finds them. Conservatively disallow a WavPack
    // signature anywhere in trailing tags (scan with bounded memory).
    if end < file_end {
        f.seek(SeekFrom::Start(end))
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        let mut tail = f.by_ref().take(file_end - end);
        let mut window = [0; 4];
        let mut used = 0;
        let mut buf = [0; 8192];
        loop {
            let n = tail.read(&mut buf).map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
            if n == 0 {
                break;
            }
            for byte in &buf[..n] {
                window.rotate_left(1);
                window[3] = *byte;
                used += 1;
                if used >= 4 && &window == b"wvpk" {
                    return Err("UPLOAD_CONTENT_MISMATCH");
                }
            }
        }
    }
    let mut pos = 0;
    let mut audio = false;
    while pos < end {
        if end - pos < 32 {
            return Err("UPLOAD_CONTENT_MISMATCH");
        }
        f.seek(SeekFrom::Start(pos))
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        let mut h = [0; 32];
        f.read_exact(&mut h)
            .map_err(|_| "UPLOAD_CONTENT_MISMATCH")?;
        let size = u32::from_le_bytes(h[4..8].try_into().unwrap()) as u64 + 8;
        let version = u16::from_le_bytes(h[8..10].try_into().unwrap());
        if &h[..4] != b"wvpk"
            || !(0x402..=0x410).contains(&version)
            || size < 32
            || size > end - pos
        {
            return Err("UPLOAD_CONTENT_MISMATCH");
        }
        let flags = u32::from_le_bytes(h[24..28].try_into().unwrap());
        if flags & (0x8 | 0x80 | 0x8000_0000) != 0 {
            return Err("UPLOAD_LOSSY_NOT_ACCEPTED");
        }
        audio |= u32::from_le_bytes(h[20..24].try_into().unwrap()) > 0;
        pos += size;
    }
    if !audio {
        return Err("UPLOAD_CONTENT_MISMATCH");
    }
    Ok(())
}

/// Decode once into FLAC and a source PCM SHA-256 concurrently, then decode
/// FLAC once to verify it. This removes the old third full decoding pass.
/// FLAC compression level for converted masters. Measured single-threaded
/// on a 4-minute 24-bit/48 kHz stereo master (2026-09): level 0 66.3 % of
/// the WAV in 2.9 s, 3 and 5 57.8 % in ~3 s, 6 56.6 % in 3.8 s, 8 56.2 % in
/// 4.6 s, 12 55.6 % in 15 s. Level 5 (the FLAC default) is where extra CPU
/// stops buying size: 8 costs ~50 % more encode time on a small shared
/// 2-vCPU host for ~3 % smaller files, and decode cost is flat across
/// levels. Size does not affect DSP ingestion; the bytes stay lossless.
const FLAC_COMPRESSION_LEVEL: &str = "5";

pub fn to_flac(src: &Path, dst: &Path, container: &str) -> ConversionResult<()> {
    let report = qc::probe_classified(src).map_err(|e| match e {
        qc::AnalyzerError::Unavailable => "UPLOAD_CONVERSION_UNAVAILABLE",
        qc::AnalyzerError::Undecodable => "UPLOAD_CONTENT_MISMATCH",
    })?;
    let streams = report["streams"]
        .as_array()
        .ok_or("UPLOAD_CONTENT_MISMATCH")?;
    if streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .count()
        != 1
        || streams
            .iter()
            .any(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
    {
        return Err("UPLOAD_CONTENT_MISMATCH");
    }
    let m = qc::audio_metrics_from_report(&report).ok_or("UPLOAD_CONTENT_MISMATCH")?;
    let accepted = match container {
        // Same integer PCM the Stage 1 WAV policy accepts at 16/24 bit
        // (float, 32-bit and 8-bit WAVs were always rejected there).
        "WAV" => {
            m.format_name == "wav" && matches!(m.codec_name.as_str(), "pcm_s16le" | "pcm_s24le")
        }
        "M4A" => m.format_name == "mov" && m.codec_name == "alac",
        "AIFF" => {
            m.format_name == "aiff"
                && matches!(
                    m.codec_name.as_str(),
                    "pcm_s16be" | "pcm_s24be" | "pcm_s16le" | "pcm_s24le"
                )
        }
        "WAVPACK" => m.format_name == "wv" && m.codec_name == "wavpack",
        "TTA" => m.format_name == "tta" && m.codec_name == "tta",
        "FLAC" => m.format_name == "flac" && m.codec_name == "flac",
        _ => false,
    };
    if !accepted {
        // Float / 32-bit / 8-bit PCM in a WAV is lossless but not a
        // deliverable format: say so, not "lossy".
        if container == "WAV" && m.format_name == "wav" && m.codec_name.starts_with("pcm_") {
            return Err("UPLOAD_AUDIO_FORMAT_UNSUPPORTED");
        }
        return Err("UPLOAD_LOSSY_NOT_ACCEPTED");
    }
    if !matches!(m.bits_per_sample, Some(16 | 24))
        || !(1..=2).contains(&m.channels)
        || !(qc::MIN_SAMPLE_RATE..=qc::MAX_SAMPLE_RATE).contains(&m.sample_rate)
    {
        return Err("UPLOAD_AUDIO_FORMAT_UNSUPPORTED");
    }
    if container == "WAVPACK" {
        validate_wavpack(src)?;
    }
    // A WAV whose data chunk promises more bytes than the file holds would
    // otherwise convert "cleanly" to a shorter FLAC and lose the evidence
    // Stage 1 used to report as AUDIO_TRUNCATED.
    if container == "WAV" && qc::wav_data_truncation(src).is_some() {
        return Err("UPLOAD_AUDIO_TRUNCATED");
    }
    // Bound expansion before launching the decoder, plus a file-size cap
    // below for forged duration headers. Never resample or downconvert.
    let pcm_bytes = m.duration_secs
        * f64::from(m.sample_rate)
        * f64::from(m.channels)
        * f64::from(m.bits_per_sample.unwrap() / 8);
    if pcm_bytes > crate::uploads::MAX_AUDIO_BYTES as f64 {
        return Err("UPLOAD_AUDIO_TOO_LARGE");
    }
    let limit = crate::uploads::MAX_AUDIO_BYTES.to_string();
    let args: Vec<&OsStr> = vec![
        "-nostdin".as_ref(),
        "-v".as_ref(),
        "error".as_ref(),
        "-xerror".as_ref(),
        "-y".as_ref(),
        "-protocol_whitelist".as_ref(),
        "file".as_ref(),
        "-threads".as_ref(),
        "1".as_ref(),
        "-i".as_ref(),
        src.as_os_str(),
        "-map".as_ref(),
        "0:a:0".as_ref(),
        "-map_metadata".as_ref(),
        "-1".as_ref(),
        "-c:a".as_ref(),
        "flac".as_ref(),
        "-threads".as_ref(),
        "1".as_ref(),
        "-compression_level".as_ref(),
        FLAC_COMPRESSION_LEVEL.as_ref(),
        "-fs".as_ref(),
        limit.as_ref(),
        "-f".as_ref(),
        "flac".as_ref(),
        dst.as_os_str(),
        "-map".as_ref(),
        "0:a:0".as_ref(),
        "-c:a".as_ref(),
        "pcm_s32le".as_ref(),
        "-f".as_ref(),
        "hash".as_ref(),
        "-hash".as_ref(),
        "sha256".as_ref(),
        "pipe:1".as_ref(),
    ];
    let original_hash = hash_output(&run(&args, Duration::from_secs(600))?)?;
    if std::fs::metadata(dst)
        .map_err(|_| "UPLOAD_CONVERSION_FAILED")?
        .len()
        > crate::uploads::MAX_AUDIO_BYTES as u64
    {
        return Err("UPLOAD_AUDIO_TOO_LARGE");
    }
    let out = qc::probe_audio_metrics(dst).ok_or("UPLOAD_CONVERSION_FAILED")?;
    if out.codec_name != "flac"
        || out.sample_rate != m.sample_rate
        || out.channels != m.channels
        || out.bits_per_sample != m.bits_per_sample
    {
        return Err("UPLOAD_CONVERSION_FAILED");
    }
    if original_hash != pcm_sha256(dst)? {
        return Err("UPLOAD_CONVERSION_NOT_LOSSLESS");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additional_lossless_formats_preserve_pcm() {
        for (ext, codec, container, bits) in [
            ("wav", "pcm_s16le", "WAV", 16),
            ("wav", "pcm_s24le", "WAV", 24),
            ("aiff", "pcm_s16be", "AIFF", 16),
            ("aiff", "pcm_s24be", "AIFF", 24),
            ("wv", "wavpack", "WAVPACK", 16),
            ("tta", "tta", "TTA", 16),
        ] {
            let base = std::env::temp_dir().join(format!("audeniq-test-{}", uuid::Uuid::new_v4()));
            let src = base.with_extension(ext);
            let dst = base.with_extension("flac");
            let status = std::process::Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=997:duration=2:sample_rate=48000",
                    "-ac",
                    "2",
                    "-c:a",
                    codec,
                ])
                .arg(&src)
                .status()
                .unwrap();
            assert!(status.success());
            assert_eq!(
                qc::detect_container(&std::fs::read(&src).unwrap()),
                container
            );
            to_flac(&src, &dst, container).unwrap();
            let out = qc::probe_audio_metrics(&dst).unwrap();
            assert_eq!(
                (out.bits_per_sample, out.channels, out.sample_rate),
                (Some(bits), 2, 48000)
            );
            assert_eq!(pcm_sha256(&src).unwrap(), pcm_sha256(&dst).unwrap());
            std::fs::remove_file(src).unwrap();
            std::fs::remove_file(dst).unwrap();
        }
    }

    #[test]
    fn wavpack_checks_every_block_and_rejects_truncation() {
        fn block(flags: u32) -> Vec<u8> {
            let mut b = vec![0; 32];
            b[..4].copy_from_slice(b"wvpk");
            b[4..8].copy_from_slice(&24u32.to_le_bytes());
            b[8..10].copy_from_slice(&0x410u16.to_le_bytes());
            b[20..24].copy_from_slice(&100u32.to_le_bytes());
            b[24..28].copy_from_slice(&flags.to_le_bytes());
            b
        }
        let p = std::env::temp_dir().join(format!("audeniq-wv-{}", uuid::Uuid::new_v4()));
        for bad_flags in [0x8, 0x80, 0x8000_0000] {
            let bytes = [block(0), block(bad_flags)].concat();
            std::fs::write(&p, bytes).unwrap();
            assert_eq!(validate_wavpack(&p), Err("UPLOAD_LOSSY_NOT_ACCEPTED"));
        }
        std::fs::write(&p, &block(0)[..31]).unwrap();
        assert_eq!(validate_wavpack(&p), Err("UPLOAD_CONTENT_MISMATCH"));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn wav_that_cannot_be_delivered_is_refused_before_conversion() {
        let make = |codec: &str| {
            let src =
                std::env::temp_dir().join(format!("audeniq-wav-{}.wav", uuid::Uuid::new_v4()));
            assert!(
                std::process::Command::new("ffmpeg")
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=duration=2:sample_rate=48000",
                        "-c:a",
                        codec,
                    ])
                    .arg(&src)
                    .status()
                    .unwrap()
                    .success()
            );
            src
        };
        for codec in ["pcm_f32le", "pcm_s32le", "pcm_u8"] {
            let src = make(codec);
            let dst = src.with_extension("flac");
            assert_eq!(
                to_flac(&src, &dst, "WAV"),
                Err("UPLOAD_AUDIO_FORMAT_UNSUPPORTED"),
                "{codec}"
            );
            assert!(!dst.exists());
            std::fs::remove_file(src).unwrap();
        }
        // Truncated: the data chunk promises more bytes than the file has.
        // Converting it would silently produce a shorter, valid FLAC.
        let src = make("pcm_s24le");
        let dst = src.with_extension("flac");
        let bytes = std::fs::read(&src).unwrap();
        std::fs::write(&src, &bytes[..bytes.len() - 48_000]).unwrap();
        assert_eq!(to_flac(&src, &dst, "WAV"), Err("UPLOAD_AUDIO_TRUNCATED"));
        assert!(!dst.exists());
        std::fs::remove_file(src).unwrap();
    }

    #[test]
    fn float_aiff_is_rejected_before_conversion() {
        let src = std::env::temp_dir().join(format!("audeniq-float-{}.aiff", uuid::Uuid::new_v4()));
        let dst = src.with_extension("flac");
        assert!(
            std::process::Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=duration=1:sample_rate=48000",
                    "-c:a",
                    "pcm_f32be"
                ])
                .arg(&src)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            to_flac(&src, &dst, "AIFF"),
            Err("UPLOAD_LOSSY_NOT_ACCEPTED")
        );
        assert!(!dst.exists());
        std::fs::remove_file(src).unwrap();
    }
}
