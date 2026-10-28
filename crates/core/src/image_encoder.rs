//! Trusted, offline derivative producer. Never called from an HTTP request.
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

/// Lossless is deliberate: no lossy quality number is silently ignored.
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
    options.validate()?;
    let source = uploads::stored_image(s, org, asset).await?;
    ensure!(source.sha256 == expected_source, "source revision mismatch");
    if let Some(row) = sqlx::query("SELECT object_key,sha256,size_bytes FROM catalog.image_variants WHERE org_id=$1 AND asset_id=$2 AND source_sha256=$3 AND content_type='image/webp'")
        .bind(org).bind(asset).bind(expected_source).fetch_optional(&s.pool).await? {
        let digest: String = row.get("sha256");
        let size: i64 = row.get("size_bytes");
        ensure!((1..=uploads::MAX_IMAGE_BYTES).contains(&size), "invalid registered variant size");
        let bytes = s.storage.get(&row.get::<String,_>("object_key")).await?;
        ensure!(crate::image_variants::verified(&bytes, size, &digest, "image/webp"), "registered variant unavailable or corrupt");
        return Ok(digest);
    }
    let dir = Scratch::new()?;
    let input = dir.0.join("source");
    let output_dir = dir.0.join("out");
    std::fs::create_dir(&output_dir)?;
    let output = output_dir.join("variant.webp");
    tokio::fs::write(&input, &source.bytes).await?;
    let mut command = Command::new(executable);
    command
        .arg("sandbox-webp")
        .arg(&input)
        .arg(&output)
        .arg(expected_source)
        .arg(options.max_edge.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::parser_sandbox::restrict(&mut command, &[&input], &[&output_dir])?;
    let status = tokio::task::spawn_blocking(move || command.status()).await??;
    ensure!(status.success(), "sandboxed WebP encoding failed");
    let size = tokio::fs::metadata(&output).await?.len();
    ensure!(
        size > 0 && size <= uploads::MAX_IMAGE_BYTES as u64,
        "variant exceeds output cap"
    );
    let bytes = tokio::fs::read(&output).await?;
    let digest = crate::domain::sha256_hex(&bytes);
    ensure!(
        crate::image_variants::verified(&bytes, size as i64, &digest, "image/webp"),
        "invalid WebP output"
    );
    // Unique key: never overwrite a source or a previously published derivative.
    let key = format!(
        "image-variants/{org}/{asset}/{expected_source}/{}.webp",
        Uuid::new_v4()
    );
    s.storage
        .put_archive(&key, ArchiveBody::Bytes(&bytes), "image/webp", None)
        .await?;
    let read_back = s.storage.get(&key).await?;
    ensure!(read_back == bytes, "variant storage verification failed");
    // Recheck safety and revision after the expensive work, before publication.
    let current = uploads::stored_image(s, org, asset).await?;
    ensure!(
        current.sha256 == expected_source,
        "source changed before publication"
    );
    let inserted: Option<String> = sqlx::query_scalar("INSERT INTO catalog.image_variants(org_id,asset_id,source_sha256,content_type,object_key,sha256,size_bytes) VALUES($1,$2,$3,'image/webp',$4,$5,$6) ON CONFLICT DO NOTHING RETURNING sha256")
        .bind(org).bind(asset).bind(expected_source).bind(&key).bind(&digest).bind(size as i64)
        .fetch_optional(&s.pool).await?;
    let winner = match inserted {
        Some(digest) => digest,
        None => sqlx::query_scalar("SELECT sha256 FROM catalog.image_variants WHERE org_id=$1 AND asset_id=$2 AND source_sha256=$3 AND content_type='image/webp'")
            .bind(org).bind(asset).bind(expected_source).fetch_one(&s.pool).await?,
    };
    ensure!(!winner.is_empty(), "variant publication failed");
    if winner != digest {
        // A concurrent producer won. Its object and row must remain untouched.
        tracing::info!(%asset, "image_variant_already_registered");
    }
    // Losing objects may be orphaned; never delete after an uncertain DB commit.
    Ok(winner)
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

fn encode(bytes: &[u8], options: WebpOptions) -> anyhow::Result<Vec<u8>> {
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
    let rgba = decoded.to_rgba8();
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
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take(uploads::MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        crate::domain::sha256_hex(&bytes) == expected,
        "source hash mismatch"
    );
    let encoded = encode(&bytes, options).context("WebP encoding")?;
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
