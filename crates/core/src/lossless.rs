//! Normalize supported lossless uploads to a DSP-compatible FLAC master.
//! Container labels alone never establish that a codec is lossless.
//!
//! Probing, decoding, FLAC encoding and the lossless proof run in the
//! sandboxed AUDENIQ engine ([`crate::qc_engine`]); no FFmpeg is involved.
use crate::qc;
use crate::qc_engine::{self, EngineError, Rejection};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

type ConversionResult<T> = std::result::Result<T, &'static str>;

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const CONVERT_TIMEOUT: Duration = Duration::from_secs(600);

/// Canonical PCM SHA-256 (interleaved, left-aligned s32le) of a full decode.
#[cfg(test)]
pub(crate) fn pcm_sha256(path: &Path) -> ConversionResult<String> {
    qc_engine::pcm_sha256(path, CONVERT_TIMEOUT).map_err(|_| "UPLOAD_CONVERSION_FAILED")
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

/// The engine's refusal of the source, as the upload error the Studio shows.
/// An M4A the engine cannot open as a single ALAC track is (in practice) AAC:
/// the engine finds no `alac` sample description in it.
fn source_rejection(container: &str, error: EngineError) -> &'static str {
    match error {
        EngineError::Unavailable => "UPLOAD_CONVERSION_UNAVAILABLE",
        EngineError::Deadline => "UPLOAD_CONVERSION_TIMEOUT",
        EngineError::Rejected(Rejection::Limit, _) => "UPLOAD_AUDIO_TOO_LARGE",
        EngineError::Rejected(Rejection::Unsupported, _) if container == "M4A" => {
            "UPLOAD_LOSSY_NOT_ACCEPTED"
        }
        EngineError::Rejected(Rejection::Invalid, detail)
            if container == "M4A" && detail.contains("missing MP4 table") =>
        {
            "UPLOAD_LOSSY_NOT_ACCEPTED"
        }
        // Float, 32-bit, 8-bit or >2-channel PCM is lossless but not a
        // deliverable format: say so, not "lossy".
        EngineError::Rejected(Rejection::Unsupported, _) => "UPLOAD_AUDIO_FORMAT_UNSUPPORTED",
        EngineError::Rejected(Rejection::Invalid, _) => "UPLOAD_CONTENT_MISMATCH",
    }
}

/// Removes the private engine staging directory however conversion ends.
struct Staging(std::path::PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Convert `src` (WAV, ALAC in M4A, AIFF, WavPack, TTA or FLAC) to a FLAC
/// master at `dst`, which must not exist yet.
///
/// One engine run decodes the source once, hashes its PCM, encodes FLAC
/// (adaptive LPC, profile 5; a FLAC source keeps its validated frames and
/// loses only its tags), re-decodes the output and publishes it only when
/// spec, frame count and PCM SHA-256 match. The backend then checks the
/// report against the probe and the output's own STREAMINFO. Never
/// resamples, changes bit depth or keeps source metadata.
pub fn to_flac(src: &Path, dst: &Path, container: &str) -> ConversionResult<()> {
    convert_to_flac(src, dst, container, false).map(drop)
}

/// [`to_flac`] that also returns the QC analysis of the audio, measured in
/// the same decode (`None` if the engine's report could not be used).
pub fn to_flac_analyzed(
    src: &Path,
    dst: &Path,
    container: &str,
) -> ConversionResult<Option<qc_engine::Analysis>> {
    convert_to_flac(src, dst, container, true)
}

fn convert_to_flac(
    src: &Path,
    dst: &Path,
    container: &str,
    analyze: bool,
) -> ConversionResult<Option<qc_engine::Analysis>> {
    if !matches!(
        container,
        "WAV" | "M4A" | "AIFF" | "WAVPACK" | "TTA" | "FLAC"
    ) {
        return Err("UPLOAD_LOSSY_NOT_ACCEPTED");
    }
    // A WAV whose data chunk promises more bytes than the file holds keeps
    // its own code; the engine would only report it as malformed.
    if container == "WAV" && qc::wav_data_truncation(src).is_some() {
        return Err("UPLOAD_AUDIO_TRUNCATED");
    }
    let report =
        qc_engine::probe(src, PROBE_TIMEOUT).map_err(|e| source_rejection(container, e))?;
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
        _ => m.format_name == "flac" && m.codec_name == "flac",
    };
    if !accepted {
        // The magic bytes said one container, the parser found another.
        return Err("UPLOAD_CONTENT_MISMATCH");
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
    // Bound expansion before launching the decoder, plus an output size cap
    // below for forged duration headers.
    let pcm_bytes = m.duration_secs
        * f64::from(m.sample_rate)
        * f64::from(m.channels)
        * f64::from(m.bits_per_sample.unwrap() / 8);
    if pcm_bytes > crate::uploads::MAX_AUDIO_BYTES as f64 {
        return Err("UPLOAD_AUDIO_TOO_LARGE");
    }
    if dst.exists() {
        return Err("UPLOAD_CONVERSION_FAILED");
    }
    // The engine's only writable grant: a new private directory next to
    // `dst`, never the upload workspace that holds the source.
    let parent = dst.parent().ok_or("UPLOAD_CONVERSION_FAILED")?;
    let staging = Staging(parent.join(format!(".flac-staging-{}", uuid::Uuid::new_v4())));
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&staging.0)
            .map_err(|_| "UPLOAD_CONVERSION_UNAVAILABLE")?;
    }
    let (staged, mut conversion) =
        qc_engine::convert(src, &staging.0, "master.flac", CONVERT_TIMEOUT, analyze).map_err(
            |e| match e {
                EngineError::Rejected(Rejection::Unsupported, _) => source_rejection(container, e),
                EngineError::Rejected(Rejection::Limit, _) => "UPLOAD_AUDIO_TOO_LARGE",
                EngineError::Rejected(Rejection::Invalid, _) => "UPLOAD_CONVERSION_FAILED",
                EngineError::Deadline => "UPLOAD_CONVERSION_TIMEOUT",
                EngineError::Unavailable => "UPLOAD_CONVERSION_UNAVAILABLE",
            },
        )?;
    let size = std::fs::symlink_metadata(&staged)
        .ok()
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .ok_or("UPLOAD_CONVERSION_FAILED")?;
    if size > crate::uploads::MAX_AUDIO_BYTES as u64 {
        return Err("UPLOAD_AUDIO_TOO_LARGE");
    }
    // The engine verified its own output; still refuse a report that does
    // not describe the probed source, or a file whose STREAMINFO disagrees.
    let expected_frames = (m.duration_secs * f64::from(m.sample_rate)).round() as u64;
    let out = &conversion.spec;
    let header = crate::audio_header::probe(&staged).ok_or("UPLOAD_CONVERSION_FAILED")?;
    if out.codec != "flac"
        || out.sample_rate != m.sample_rate
        || out.channels != m.channels
        || Some(out.bits_per_sample) != m.bits_per_sample
        || out.frames != conversion.frames
        || conversion.frames.abs_diff(expected_frames) > u64::from(m.sample_rate)
        || conversion.output_bytes != size
        || conversion.pcm_sha256.len() != 64
        || header.codec_name != "flac"
        || header.sample_rate != m.sample_rate
        || header.channels != m.channels
        || header.bits_per_sample != m.bits_per_sample
    {
        return Err("UPLOAD_CONVERSION_NOT_LOSSLESS");
    }
    // No-clobber publication outside the engine's grant.
    std::fs::hard_link(&staged, dst).map_err(|_| "UPLOAD_CONVERSION_FAILED")?;
    Ok(conversion.analysis.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_pcm_sha256(path: &Path) -> String {
        let out = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args([
                "-map",
                "0:a:0",
                "-c:a",
                "pcm_s32le",
                "-f",
                "hash",
                "-hash",
                "sha256",
                "-",
            ])
            .output()
            .expect("ffmpeg (test oracle) missing");
        assert!(out.status.success());
        String::from_utf8(out.stdout)
            .unwrap()
            .trim()
            .to_ascii_lowercase()
    }

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
            // Independent oracle: FFmpeg (test-only) decodes both to the same PCM.
            assert_eq!(
                ffmpeg_pcm_sha256(&src),
                ffmpeg_pcm_sha256(&dst),
                "{ext} {codec}"
            );
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
            Err("UPLOAD_AUDIO_FORMAT_UNSUPPORTED")
        );
        assert!(!dst.exists());
        std::fs::remove_file(src).unwrap();
    }
}
