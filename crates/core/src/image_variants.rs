//! Negotiation for pre-generated derivatives. No conversion runs in the API.
//! A trusted processor must register derivatives against the sanitized source
//! digest; until one does, all clients receive the original JPEG/PNG.
use crate::{api::AppState, uploads::StoredImage};
use axum::http::{HeaderMap, header};
use sqlx::Row;

/// Explicit opt-in only. A wildcard is insufficient evidence of decoder support.
pub(crate) fn quality(accept: Option<&str>, format: &str) -> f32 {
    explicit_quality(accept, format).unwrap_or(0.0)
}

fn explicit_quality(accept: Option<&str>, format: &str) -> Option<f32> {
    let wanted = format!("image/{format}");
    let mut quality: Option<f32> = None;
    for part in accept.unwrap_or("").split(',') {
        let mut parts = part.split(';').map(str::trim);
        if !parts
            .next()
            .is_some_and(|mime| mime.eq_ignore_ascii_case(&wanted))
        {
            continue;
        }
        let mut q = 1.0_f32;
        for parameter in parts {
            if let Some((key, value)) = parameter.split_once('=') {
                if key.trim().eq_ignore_ascii_case("q") {
                    q = q.min(
                        value
                            .trim()
                            .parse::<f32>()
                            .ok()
                            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                            .unwrap_or(0.0),
                    );
                }
            }
        }
        quality = Some(quality.map_or(q, |old| old.min(q)));
    }
    quality
}

/// An AVIF opt-in permits the requested WebP fallback if AVIF is unavailable.
/// An explicit WebP exclusion always wins; wildcards alone never opt in.
pub(crate) fn accepts_variant(accept: Option<&str>, format: &str) -> bool {
    quality(accept, format) > 0.0
        || (format == "webp"
            && quality(accept, "avif") > 0.0
            && explicit_quality(accept, "webp").is_none())
}

/// Parent authorization and safety/hash checks have already run. Derivative
/// lookup is bound to that exact source hash, never merely a mutable alias.
/// Missing/unavailable derivatives preserve the source response.
pub async fn negotiate(s: &AppState, mut image: StoredImage, h: &HeaderMap) -> StoredImage {
    let accept = h.get(header::ACCEPT).and_then(|v| v.to_str().ok());
    for format in ["avif", "webp"] {
        if !accepts_variant(accept, format) {
            continue;
        }
        let mime = if format == "avif" {
            "image/avif"
        } else {
            "image/webp"
        };
        let row = sqlx::query("SELECT object_key, sha256, size_bytes FROM catalog.image_variants WHERE org_id=$1 AND asset_id=$2 AND source_sha256=$3 AND content_type=$4")
            .bind(image.org_id).bind(image.asset_id).bind(&image.sha256).bind(mime)
            .fetch_optional(&s.pool).await;
        let row = match row {
            Ok(Some(row)) => row,
            Ok(None) => continue,
            Err(_) => {
                tracing::warn!(asset = %image.asset_id, format, "image_variant_lookup_unavailable");
                continue;
            }
        };
        let digest: String = row.get("sha256");
        let size: i64 = row.get("size_bytes");
        if let Ok(bytes) = s.storage.get(&row.get::<String, _>("object_key")).await {
            if verified(&bytes, size, &digest, mime) {
                image.bytes = bytes;
                image.sha256 = digest;
                image.content_type = mime;
                return image;
            }
        }
        tracing::warn!(asset = %image.asset_id, format, "image_variant_unavailable_or_unverified");
    }
    image
}

pub(crate) fn verified(bytes: &[u8], size: i64, digest: &str, mime: &str) -> bool {
    let magic = match mime {
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/avif" => bytes.get(4..8) == Some(b"ftyp") && bytes.get(8..12) == Some(b"avif"),
        _ => false,
    };
    size > 0
        && size <= crate::uploads::MAX_IMAGE_BYTES
        && bytes.len() as i64 == size
        && magic
        && crate::domain::sha256_hex(bytes) == digest
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avif_falls_back_to_webp_unless_explicitly_excluded() {
        assert!(accepts_variant(Some("image/avif"), "webp"));
        assert!(accepts_variant(Some("image/avif,image/webp"), "webp"));
        for value in [
            "*/*",
            "image/avif;q=0",
            "image/avif,image/webp;q=0",
            "image/avif,image/webp;q=broken",
        ] {
            assert!(!accepts_variant(Some(value), "webp"));
        }
    }
    #[test]
    fn explicit_formats_and_quality_exclusions() {
        for value in [
            None,
            Some("*/*"),
            Some("image/*"),
            Some("image/avif;q=0"),
            Some("image/avif;q=NaN"),
            Some("image/avif;q=broken"),
            Some("image/avif;q=2"),
            Some("image/avif;q=0;q=1"),
        ] {
            assert_eq!(quality(value, "avif"), 0.0);
        }
        assert_eq!(quality(Some("IMAGE/AVIF; Q=0.8, image/webp"), "avif"), 0.8);
        assert_eq!(quality(Some("image/avif, image/avif;q=0"), "avif"), 0.0);
        assert_eq!(quality(Some("image/webp"), "webp"), 1.0);
    }
    #[test]
    fn derivatives_require_exact_digest_size_and_format() {
        let bytes = b"RIFF1234WEBPfixture";
        let sha = crate::domain::sha256_hex(bytes);
        assert!(verified(bytes, bytes.len() as i64, &sha, "image/webp"));
        assert!(!verified(bytes, bytes.len() as i64, &sha, "image/avif"));
        assert!(!verified(bytes, 1, &sha, "image/webp"));
        assert!(!verified(
            bytes,
            bytes.len() as i64,
            &"0".repeat(64),
            "image/webp"
        ));
    }
}
