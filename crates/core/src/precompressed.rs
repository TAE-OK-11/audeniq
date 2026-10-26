//! Large bodies that many requests repeat byte for byte (the MCP tool list,
//! about 60 KB) compressed once, at the maximum levels, instead of at level 4
//! on every request by the API's compression layer: brotli alone cost about
//! 0.8 ms of CPU per `tools/list`. A handler marks such a response with
//! [`Repeated`]; [`apply`] serves the cached copy for the client's encoding
//! and sets `Content-Encoding`, which the compression layer then leaves alone.
//! Anything else (another encoding, a streamed body) is passed through.

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use std::io::Write;
use std::sync::Mutex;

/// Response extension: this body is repeated byte for byte across requests.
#[derive(Clone, Copy)]
pub struct Repeated;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Br,
    Gzip,
}

/// Distinct bodies kept (each tool list × JSON-RPC id × revision × encoding);
/// clients number their first requests alike, so a few cover nearly all.
const CAPACITY: usize = 32;
/// Bodies at most this large (the full tool list is well under it).
const MAX_BODY: usize = 512 * 1024;

struct Entry {
    encoding: Encoding,
    plain: Bytes,
    packed: Bytes,
}

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// The encoding the compression layer would pick from `Accept-Encoding`
/// (highest q-value; brotli on a tie), or `None` to leave the body to it.
fn negotiate(h: &HeaderMap) -> Option<Encoding> {
    let mut br = None;
    let mut gzip = None;
    let mut wildcard = None;
    for v in h.get_all(header::ACCEPT_ENCODING) {
        for item in v.to_str().ok()?.split(',') {
            let mut parts = item.split(';');
            let name = parts.next()?.trim();
            let mut q = 1000u16;
            for p in parts {
                let p = p.trim();
                let value = p.strip_prefix("q=").or_else(|| p.strip_prefix("Q="))?;
                let f: f32 = value.parse().ok()?;
                if !(0.0..=1.0).contains(&f) {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    q = (f * 1000.0).round() as u16;
                }
            }
            let slot = if name.eq_ignore_ascii_case("br") {
                &mut br
            } else if name.eq_ignore_ascii_case("gzip") {
                &mut gzip
            } else if name == "*" {
                &mut wildcard
            } else {
                continue;
            };
            *slot = Some(q);
        }
    }
    let br = br.or(wildcard).unwrap_or(0);
    let gzip = gzip.or(wildcard).unwrap_or(0);
    match (br, gzip) {
        (0, 0) => None,
        (b, g) if b >= g => Some(Encoding::Br),
        _ => Some(Encoding::Gzip),
    }
}

fn compress(encoding: Encoding, plain: &[u8]) -> Bytes {
    let mut out = Vec::with_capacity(plain.len() / 4);
    match encoding {
        Encoding::Br => {
            let mut w = brotli::CompressorWriter::new(&mut out, 4096, 11, 22);
            w.write_all(plain).expect("in-memory write");
        }
        Encoding::Gzip => {
            let mut w = flate2::write::GzEncoder::new(&mut out, flate2::Compression::best());
            w.write_all(plain).expect("in-memory write");
            w.finish().expect("in-memory write");
        }
    }
    out.into()
}

fn lookup(encoding: Encoding, plain: &[u8]) -> Option<Bytes> {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .iter()
        .find(|e| e.encoding == encoding && e.plain.len() == plain.len() && *e.plain == *plain)
        .map(|e| e.packed.clone())
}

/// `r` compressed from the cache when it is marked [`Repeated`] and the
/// client takes br or gzip; otherwise `r` as it is.
pub async fn apply(accept: &HeaderMap, r: Response) -> Response {
    if r.extensions().get::<Repeated>().is_none()
        || r.headers().contains_key(header::CONTENT_ENCODING)
    {
        return r;
    }
    let Some(encoding) = negotiate(accept) else {
        return r;
    };
    let (mut parts, body) = r.into_parts();
    let Ok(plain) = axum::body::to_bytes(body, MAX_BODY).await else {
        return crate::error::Error::Internal.into_response();
    };
    let packed = match lookup(encoding, &plain) {
        Some(p) => p,
        None => {
            let p = {
                let plain = plain.clone();
                tokio::task::spawn_blocking(move || compress(encoding, &plain))
                    .await
                    .unwrap_or_default()
            };
            if p.is_empty() {
                return Response::from_parts(parts, Body::from(plain));
            }
            let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() >= CAPACITY {
                cache.remove(0);
            }
            cache.push(Entry {
                encoding,
                plain,
                packed: p.clone(),
            });
            p
        }
    };
    parts.headers.insert(
        header::CONTENT_ENCODING,
        HeaderValue::from_static(match encoding {
            Encoding::Br => "br",
            Encoding::Gzip => "gzip",
        }),
    );
    parts
        .headers
        .append(header::VARY, HeaderValue::from_static("accept-encoding"));
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.remove(header::ACCEPT_RANGES);
    Response::from_parts(parts, Body::from(packed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn accept(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT_ENCODING, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn negotiates_like_the_compression_layer() {
        assert_eq!(negotiate(&accept("gzip, deflate, br")), Some(Encoding::Br));
        assert_eq!(negotiate(&accept("gzip")), Some(Encoding::Gzip));
        assert_eq!(negotiate(&accept("br;q=0.5, gzip")), Some(Encoding::Gzip));
        assert_eq!(negotiate(&accept("br;q=0, gzip;q=0")), None);
        assert_eq!(negotiate(&accept("*")), Some(Encoding::Br));
        assert_eq!(negotiate(&accept("*;q=0.2, br;q=0")), Some(Encoding::Gzip));
        assert_eq!(negotiate(&accept("identity")), None);
        assert_eq!(negotiate(&accept("br;level=3")), None);
        assert_eq!(negotiate(&HeaderMap::new()), None);
    }

    async fn served(h: &HeaderMap, mark: bool, plain: &'static str) -> (HeaderMap, Bytes) {
        let mut r = Response::new(Body::from(plain));
        if mark {
            r.extensions_mut().insert(Repeated);
        }
        let r = apply(h, r).await;
        let headers = r.headers().clone();
        (
            headers,
            axum::body::to_bytes(r.into_body(), MAX_BODY).await.unwrap(),
        )
    }

    #[tokio::test]
    async fn repeated_bodies_decode_to_the_original() {
        let plain: &'static str = Box::leak(
            "{\"tools\":[\"precompressed round trip\"]}"
                .repeat(200)
                .into_boxed_str(),
        );
        for _ in 0..2 {
            let (h, br) = served(&accept("br, gzip"), true, plain).await;
            assert_eq!(h[header::CONTENT_ENCODING], "br");
            assert_eq!(h[header::VARY], "accept-encoding");
            let mut out = String::new();
            brotli::Decompressor::new(&br[..], 4096)
                .read_to_string(&mut out)
                .unwrap();
            assert_eq!(out, plain);
            let (h, gz) = served(&accept("gzip"), true, plain).await;
            assert_eq!(h[header::CONTENT_ENCODING], "gzip");
            let mut out = String::new();
            flate2::read::GzDecoder::new(&gz[..])
                .read_to_string(&mut out)
                .unwrap();
            assert_eq!(out, plain);
        }
        // Unmarked, or a client without br/gzip: passed through untouched.
        let (h, b) = served(&accept("br"), false, plain).await;
        assert!(h.get(header::CONTENT_ENCODING).is_none());
        assert_eq!(b, plain.as_bytes());
        let (h, b) = served(&accept("identity"), true, plain).await;
        assert!(h.get(header::CONTENT_ENCODING).is_none());
        assert_eq!(b, plain.as_bytes());
    }
}
