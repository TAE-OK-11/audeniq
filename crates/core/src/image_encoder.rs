//! Shared offline/background derivative producer. Never called from an HTTP request.
//! The parent obtains a safety-verified source; all codec work runs in a
//! credential-free parser sandbox. Publication follows an object read-back.
use crate::{api::AppState, storage::ArchiveBody, uploads};
use anyhow::{Context, ensure};
use image::{ImageDecoder, ImageEncoder, ImageFormat, ImageReader};
use sqlx::Row;
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use uuid::Uuid;

const MAX_EDGE: u32 = 8192;
const MAX_PIXELS: u64 = 16_777_216;
// See docs/image-pipeline/phase-2.md for the measured quality/time tradeoff.
const WEBP_QUALITY: f32 = 95.0;
const AVIF_QUALITY: f32 = 90.0;
const AVIF_SPEED: u8 = 8;
const AVIF_MAX_PIXELS: u64 = 2_250_000;

/// Geometry shared by the codecs; quality is selected by `Codec` and the
/// measured constants above. Only the offline lossless CLI exposes resizing.
/// A smaller max_edge explicitly opts into Lanczos3 resizing. Default: unchanged.
#[derive(Clone, Copy)]
pub struct WebpOptions {
    pub max_edge: u32,
}
impl Default for WebpOptions {
    fn default() -> Self {
        Self { max_edge: MAX_EDGE }
    }
}
impl WebpOptions {
    fn validate(self) -> anyhow::Result<()> {
        ensure!(
            (1..=MAX_EDGE).contains(&self.max_edge),
            "max-edge must be 1..=8192"
        );
        Ok(())
    }
}

/// Lossless remains available for the explicit offline operator interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    LosslessWebp,
    LossyWebp,
    Avif,
}
impl Codec {
    fn mime(self) -> &'static str {
        if self == Self::Avif {
            "image/avif"
        } else {
            "image/webp"
        }
    }
    fn extension(self) -> &'static str {
        if self == Self::Avif { "avif" } else { "webp" }
    }
    fn command(self) -> &'static str {
        match self {
            Self::LosslessWebp => "sandbox-webp",
            Self::LossyWebp => "sandbox-lossy-webp",
            Self::Avif => "sandbox-avif",
        }
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!("audeniq-image-{}", Uuid::new_v4()));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The executable must be the trusted audeniq-image-encode binary, never a
/// request parameter. Caller is an offline operator with worker DB credentials.
/// Returns the winning row's digest (repeated/concurrent runs are write-once).
pub async fn produce_webp(
    s: &AppState,
    org: Uuid,
    asset: Uuid,
    expected_source: &str,
    options: WebpOptions,
    executable: &Path,
) -> anyhow::Result<String> {
    produce_webp_with(
        &s.pool,
        &s.storage,
        s.antivirus_enabled,
        org,
        asset,
        expected_source,
        options,
        executable,
        false,
    )
    .await?
    .context("offline encoding unexpectedly skipped")
}

/// Minimum 10% AND 1 KiB saving avoids storing marginally useful duplicates.
pub(crate) fn worth_storing(source: usize, variant: usize) -> bool {
    source >= 1024
        && variant > 0
        && variant <= source.saturating_sub(1024)
        && (variant as u64) * 10 <= (source as u64) * 9
}

/// Same sandbox, validation and write-once publisher as the offline producer.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn produce_webp_with(
    pool: &sqlx::PgPool,
    storage: &std::sync::Arc<dyn crate::storage::ObjectStore>,
    antivirus_enabled: bool,
    org: Uuid,
    asset: Uuid,
    expected_source: &str,
    options: WebpOptions,
    executable: &Path,
    only_if_useful: bool,
) -> anyhow::Result<Option<String>> {
    produce_variant_with(
        pool,
        storage,
        antivirus_enabled,
        org,
        asset,
        expected_source,
        options,
        executable,
        only_if_useful,
        Codec::LosslessWebp,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn produce_variant_with(
    pool: &sqlx::PgPool,
    storage: &std::sync::Arc<dyn crate::storage::ObjectStore>,
    antivirus_enabled: bool,
    org: Uuid,
    asset: Uuid,
    expected_source: &str,
    options: WebpOptions,
    executable: &Path,
    only_if_useful: bool,
    codec: Codec,
) -> anyhow::Result<Option<String>> {
    options.validate()?;
    let source = uploads::stored_image_with(pool, storage, antivirus_enabled, org, asset).await?;
    ensure!(source.sha256 == expected_source, "source revision mismatch");
    if let Some(row) = sqlx::query("SELECT object_key,sha256,size_bytes FROM catalog.image_variants WHERE org_id=$1 AND asset_id=$2 AND source_sha256=$3 AND content_type=$4")
        .bind(org).bind(asset).bind(expected_source).bind(codec.mime()).fetch_optional(pool).await? {
        let digest: String = row.get("sha256");
        let size: i64 = row.get("size_bytes");
        ensure!((1..=uploads::MAX_IMAGE_BYTES).contains(&size), "invalid registered variant size");
        let bytes = storage.get(&row.get::<String,_>("object_key")).await?;
        ensure!(crate::image_variants::verified(&bytes, size, &digest, codec.mime()), "registered variant unavailable or corrupt");
        return Ok(Some(digest));
    }
    // Even a zero-byte variant cannot meet the minimum saving for tiny files.
    if only_if_useful && source.bytes.len() <= 1024 {
        return Ok(None);
    }
    let dir = Scratch::new()?;
    let input = dir.0.join("source");
    let output_dir = dir.0.join("out");
    std::fs::create_dir(&output_dir)?;
    let output = output_dir.join(format!("variant.{}", codec.extension()));
    tokio::fs::write(&input, &source.bytes).await?;
    let mut command = Command::new(executable);
    command
        .arg(codec.command())
        .arg(&input)
        .arg(&output)
        .arg(expected_source)
        .arg(options.max_edge.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::parser_sandbox::restrict(&mut command, &[&input], &[&output_dir])?;
    configure_codec_child(&mut command);
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .spawn()?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(120), child.wait())
        .await
        .context("image codec wall timeout")??;
    ensure!(status.success(), "sandboxed image encoding failed");
    let size = tokio::fs::metadata(&output).await?.len();
    if only_if_useful && size == 0 {
        return Ok(None);
    }
    ensure!(
        size > 0 && size <= uploads::MAX_IMAGE_BYTES as u64,
        "variant exceeds output cap"
    );
    // The automatic path never resizes and only publishes meaningful savings.
    // JPEG/PNG are the only admitted sources, so WebP/AVIF are never recompressed.
    if only_if_useful && !worth_storing(source.bytes.len(), size as usize) {
        return Ok(None);
    }
    let bytes = tokio::fs::read(&output).await?;
    let digest = crate::domain::sha256_hex(&bytes);
    ensure!(
        crate::image_variants::verified(&bytes, size as i64, &digest, codec.mime()),
        "invalid image output"
    );
    // Unique key: never overwrite a source or a previously published derivative.
    let key = format!(
        "image-variants/{org}/{asset}/{expected_source}/{}.{}",
        Uuid::new_v4(),
        codec.extension()
    );
    storage
        .put_archive(&key, ArchiveBody::Bytes(&bytes), codec.mime(), None)
        .await?;
    let read_back = storage.get(&key).await?;
    ensure!(read_back == bytes, "variant storage verification failed");
    // Recheck safety and revision after the expensive work, before publication.
    let current = uploads::stored_image_with(pool, storage, antivirus_enabled, org, asset).await?;
    ensure!(
        current.sha256 == expected_source,
        "source changed before publication"
    );
    let inserted: Option<String> = sqlx::query_scalar("INSERT INTO catalog.image_variants(org_id,asset_id,source_sha256,content_type,object_key,sha256,size_bytes) VALUES($1,$2,$3,$7,$4,$5,$6) ON CONFLICT DO NOTHING RETURNING sha256")
        .bind(org).bind(asset).bind(expected_source).bind(&key).bind(&digest).bind(size as i64).bind(codec.mime())
        .fetch_optional(pool).await?;
    let winner = match inserted {
        Some(digest) => digest,
        None => sqlx::query_scalar("SELECT sha256 FROM catalog.image_variants WHERE org_id=$1 AND asset_id=$2 AND source_sha256=$3 AND content_type=$4")
            .bind(org).bind(asset).bind(expected_source).bind(codec.mime()).fetch_one(pool).await?,
    };
    ensure!(!winner.is_empty(), "variant publication failed");
    if winner != digest {
        // A concurrent producer won. Its object and row must remain untouched.
        tracing::info!(%asset, "image_variant_already_registered");
    }
    // Losing objects may be orphaned; never delete after an uncertain DB commit.
    Ok(Some(winner))
}

/// Drop/timeout cancellation and abrupt worker death must both stop codec work.
/// Otherwise a released advisory lock could allow a second conversion alongside
/// an orphan. The parent check closes the fork-to-prctl death race.
fn configure_codec_child(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    #[cfg(target_os = "linux")]
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: the post-fork hook only invokes syscalls with captured scalar data.
    unsafe {
        command.pre_exec(move || {
            #[cfg(target_os = "linux")]
            {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
                }
            }
            if libc::setpriority(libc::PRIO_PROCESS, 0, 19) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

struct Capped(Vec<u8>);
impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() > uploads::MAX_IMAGE_BYTES as usize - self.0.len() {
            return Err(std::io::Error::other("variant exceeds 20 MiB"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn reader(bytes: &[u8], format: ImageFormat) -> ImageReader<Cursor<&[u8]>> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader
}

#[cfg(test)]
fn encode(bytes: &[u8], options: WebpOptions) -> anyhow::Result<Vec<u8>> {
    encode_variant(bytes, options, Codec::LosslessWebp)
}

fn encode_variant(bytes: &[u8], options: WebpOptions, codec: Codec) -> anyhow::Result<Vec<u8>> {
    options.validate()?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= uploads::MAX_IMAGE_BYTES as usize,
        "invalid source size"
    );
    let format = image::guess_format(bytes)?;
    ensure!(
        matches!(format, ImageFormat::Jpeg | ImageFormat::Png),
        "source must be sanitized JPEG/PNG"
    );
    let (w, h) = reader(bytes, format).into_dimensions()?;
    ensure!(
        u64::from(w) * u64::from(h) <= MAX_PIXELS,
        "source exceeds pixel limit"
    );
    // Reject optional AVIF work before allocating/decoding the source pixels.
    if codec == Codec::Avif && u64::from(w) * u64::from(h) > AVIF_MAX_PIXELS {
        return Ok(Vec::new());
    }
    let mut decoder = reader(bytes, format).into_decoder()?;
    let icc = decoder.icc_profile()?;
    let decoded = image::DynamicImage::from_decoder(decoder)?;
    // Avoid silently discarding 16-bit precision. Sanitizer must first produce 8-bit.
    ensure!(
        decoded.color().bits_per_pixel() / u16::from(decoded.color().channel_count()) <= 8,
        "16-bit source is unsupported"
    );
    let decoded = if w > options.max_edge || h > options.max_edge {
        decoded.resize(
            options.max_edge,
            options.max_edge,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        decoded
    };
    // Lossy outputs do not carry arbitrary color profiles. Preserve the source
    // rather than silently treating a non-sRGB profile as sRGB.
    let profiled = icc.as_ref().is_some_and(|v| !v.is_empty());
    if codec == Codec::Avif && profiled {
        return Ok(Vec::new());
    }
    let codec = if profiled { Codec::LosslessWebp } else { codec };
    let rgba = decoded.to_rgba8();
    if codec == Codec::LossyWebp {
        let encoded = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height())
            .encode_simple(false, WEBP_QUALITY)
            .map_err(|e| anyhow::anyhow!("WebP encoding: {e:?}"))?
            .to_vec();
        ensure!(
            encoded.len() <= uploads::MAX_IMAGE_BYTES as usize,
            "WebP output cap"
        );
        let roundtrip = reader(&encoded, ImageFormat::WebP).decode()?.to_rgba8();
        ensure!(
            roundtrip.dimensions() == rgba.dimensions(),
            "WebP dimensions mismatch"
        );
        ensure!(
            roundtrip
                .pixels()
                .zip(rgba.pixels())
                .all(|(a, b)| a[3] == b[3]),
            "WebP alpha mismatch"
        );
        return Ok(encoded);
    }
    if codec == Codec::Avif {
        let pixels: Vec<ravif::RGBA8> = rgba
            .pixels()
            .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
            .collect();
        let encoded = ravif::Encoder::new()
            .with_quality(AVIF_QUALITY)
            .with_alpha_quality(100.0)
            .with_speed(AVIF_SPEED)
            .with_num_threads(Some(1))
            .encode_rgba(ravif::Img::new(
                &pixels,
                rgba.width() as usize,
                rgba.height() as usize,
            ))?
            .avif_file;
        ensure!(
            encoded.len() <= uploads::MAX_IMAGE_BYTES as usize,
            "AVIF output cap"
        );
        // No AVIF decoder is in the supplied offline crate set. Signature/hash,
        // storage read-back and source-revision checks still run in the parent.
        return Ok(encoded);
    }
    let mut output = Capped(Vec::new());
    let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut output);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc)?;
    }
    encoder.encode(
        &rgba,
        rgba.width(),
        rgba.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    let roundtrip = reader(&output.0, ImageFormat::WebP).decode()?.to_rgba8();
    ensure!(roundtrip == rgba, "WebP roundtrip pixel mismatch");
    Ok(output.0)
}

/// Internal child entry. Parent is responsible for safety-row verification.
#[doc(hidden)]
pub fn sandbox_webp(
    input: &Path,
    output: &Path,
    expected: &str,
    options: WebpOptions,
) -> anyhow::Result<()> {
    sandbox_variant(input, output, expected, options, Codec::LosslessWebp)
}

#[doc(hidden)]
pub fn sandbox_variant(
    input: &Path,
    output: &Path,
    expected: &str,
    options: WebpOptions,
    codec: Codec,
) -> anyhow::Result<()> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take(uploads::MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        crate::domain::sha256_hex(&bytes) == expected,
        "source hash mismatch"
    );
    let encoded = encode_variant(&bytes, options, codec).context("image encoding")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&encoded)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let image = image::RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([
                (x * 7) as u8,
                (y * 9) as u8,
                120,
                if x % 2 == 0 { 0 } else { 255 },
            ])
        });
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn codec_child_lifetime_and_priority() {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--list");
        configure_codec_child(&mut command);
        // Inspect the forking thread: libtest runs test bodies on new threads,
        // and Linux parent-death settings are thread-specific.
        unsafe {
            command.pre_exec(|| {
                let mut signal: libc::c_int = 0;
                if libc::prctl(libc::PR_GET_PDEATHSIG, &mut signal, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if signal != libc::SIGKILL || libc::getpriority(libc::PRIO_PROCESS, 0) != 19 {
                    return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
                }
                Ok(())
            });
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "status={}\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn lossy_webp_decodes_with_exact_alpha() {
        let source = png();
        let result = encode_variant(&source, WebpOptions::default(), Codec::LossyWebp).unwrap();
        let decoded = reader(&result, ImageFormat::WebP)
            .decode()
            .unwrap()
            .to_rgba8();
        let original = reader(&source, ImageFormat::Png)
            .decode()
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), original.dimensions());
        assert!(
            decoded
                .pixels()
                .zip(original.pixels())
                .all(|(a, b)| a[3] == b[3])
        );
    }

    #[test]
    fn avif_output_has_verified_signature_but_is_not_decoder_verified() {
        let result = encode_variant(&png(), WebpOptions::default(), Codec::Avif).unwrap();
        let digest = crate::domain::sha256_hex(&result);
        assert!(crate::image_variants::verified(
            &result,
            result.len() as i64,
            &digest,
            "image/avif"
        ));
        assert!(!crate::image_variants::verified(
            &result,
            result.len() as i64,
            &"0".repeat(64),
            "image/avif"
        ));
    }

    #[test]
    fn avif_skips_large_or_profiled_sources_and_webp_preserves_profile() {
        let mut large = Cursor::new(Vec::new());
        image::RgbImage::new(1501, 1500)
            .write_to(&mut large, ImageFormat::Png)
            .unwrap();
        assert!(
            encode_variant(large.get_ref(), WebpOptions::default(), Codec::Avif)
                .unwrap()
                .is_empty()
        );
        let pixels = image::RgbaImage::new(8, 8);
        let profile = b"test profile";
        let mut source = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut source);
        encoder.set_icc_profile(profile.to_vec()).unwrap();
        encoder
            .write_image(&pixels, 8, 8, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert!(
            encode_variant(&source, WebpOptions::default(), Codec::Avif)
                .unwrap()
                .is_empty()
        );
        let result = encode_variant(&source, WebpOptions::default(), Codec::LossyWebp).unwrap();
        let mut decoder = reader(&result, ImageFormat::WebP).into_decoder().unwrap();
        assert_eq!(decoder.icc_profile().unwrap().unwrap(), profile);
        assert_eq!(
            image::DynamicImage::from_decoder(decoder)
                .unwrap()
                .to_rgba8(),
            pixels
        );
    }

    #[test]
    fn saving_requires_both_absolute_and_relative_gain() {
        assert!(!worth_storing(0, 0));
        assert!(!worth_storing(1024, 1));
        assert!(!worth_storing(10_000, 9000)); // 10%, but less than 1 KiB
        assert!(!worth_storing(100_000, 99_000));
        assert!(worth_storing(10_240, 9216));
        assert!(worth_storing(4096, 3072));
        assert!(!worth_storing(4096, 4096));
        assert!(!worth_storing(4096, 5000));
    }

    #[test]
    fn webp_preserves_pixels_and_alpha_and_explicit_resize() {
        let source = png();
        let encoded = encode(&source, WebpOptions::default()).unwrap();
        assert_eq!(
            reader(&encoded, ImageFormat::WebP)
                .decode()
                .unwrap()
                .to_rgba8(),
            reader(&source, ImageFormat::Png)
                .decode()
                .unwrap()
                .to_rgba8()
        );
        let small = encode(&source, WebpOptions { max_edge: 16 }).unwrap();
        assert_eq!(
            reader(&small, ImageFormat::WebP).into_dimensions().unwrap(),
            (16, 12)
        );
    }
    #[test]
    fn jpeg_roundtrip_preserves_the_decoded_source() {
        let pixels = image::RgbImage::from_fn(48, 32, |x, y| {
            image::Rgb([(x * 5) as u8, (y * 7) as u8, 90])
        });
        let mut source = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut source, 90)
            .encode_image(&pixels)
            .unwrap();
        let result = encode(&source, WebpOptions::default()).unwrap();
        assert_eq!(
            reader(&result, ImageFormat::WebP)
                .decode()
                .unwrap()
                .to_rgba8(),
            reader(&source, ImageFormat::Jpeg)
                .decode()
                .unwrap()
                .to_rgba8()
        );
    }
    #[test]
    fn rejects_excess_dimensions_and_sixteen_bit_precision() {
        let mut source = Cursor::new(Vec::new());
        image::RgbaImage::new(MAX_EDGE + 1, 1)
            .write_to(&mut source, ImageFormat::Png)
            .unwrap();
        assert!(encode(source.get_ref(), WebpOptions::default()).is_err());
        let mut source = Cursor::new(Vec::new());
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::new(1, 1)
            .write_to(&mut source, ImageFormat::Png)
            .unwrap();
        assert!(encode(source.get_ref(), WebpOptions::default()).is_err());
    }
    #[test]
    fn refuses_invalid_input_options_and_caps_output() {
        assert!(encode(b"not PNG", WebpOptions::default()).is_err());
        assert!(encode(&png(), WebpOptions { max_edge: 0 }).is_err());
        let mut out = Capped(vec![0; uploads::MAX_IMAGE_BYTES as usize]);
        assert!(out.write_all(b"x").is_err());
        let dir = Scratch::new().unwrap();
        let input = dir.0.join("source");
        let output = dir.0.join("out");
        std::fs::write(&input, png()).unwrap();
        assert!(sandbox_webp(&input, &output, &"0".repeat(64), WebpOptions::default()).is_err());
        assert!(!output.exists());
    }
}
