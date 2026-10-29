//! Public image aliases resolve to retained content revisions. Callers must
//! check the page's visibility before invoking this module, including on a
//! revision hit. No S3 keys, credentials or public bucket grants reach clients.
use crate::{
    api::AppState,
    error::{Error, Result},
    uploads,
};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Default, Deserialize)]
pub struct RevisionQuery {
    pub revision: Option<String>,
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn redirect(path: &str, sha256: &str) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
    // Paths come from validated server route parameters; sha256 is server computed.
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&format!("{path}?revision={sha256}")).expect("validated image path"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Stable alias -> 307, revision -> original bytes. Older revision mappings
/// are never overwritten when an artist selects a new image. Public content
/// is still private in browser caches: /api never enters a shared CDN cache.
#[allow(clippy::too_many_arguments)]
pub async fn respond(
    s: &AppState,
    org: Uuid,
    scope: Uuid,
    kind: &'static str,
    current: Option<Uuid>,
    path: &str,
    query: RevisionQuery,
    headers: &HeaderMap,
) -> Result<Response> {
    if let Some(sha) = query.revision {
        if !valid_digest(&sha) {
            return Err(Error::NotFound);
        }
        let asset: Uuid = sqlx::query_scalar(
            "SELECT asset_id FROM portal.image_revisions WHERE kind=$1 AND scope_id=$2 AND org_id=$3 AND sha256=$4")
            .bind(kind).bind(scope).bind(org).bind(&sha)
            .fetch_optional(&s.pool).await?.ok_or(Error::NotFound)?;
        let image = uploads::stored_image(s, org, asset).await?;
        // Never label different bytes with the requested immutable identity.
        if image.sha256 != sha {
            return Err(Error::NotFound);
        }
        let image = crate::image_variants::negotiate(s, image, headers).await;
        return Ok(uploads::immutable_image_response(image, headers));
    }
    let asset = current.ok_or(Error::NotFound)?;
    let image = uploads::stored_image(s, org, asset).await?;
    sqlx::query("INSERT INTO portal.image_revisions(kind,scope_id,org_id,asset_id,sha256) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(kind).bind(scope).bind(org).bind(asset).bind(&image.sha256)
        .execute(&s.pool).await?;
    Ok(redirect(path, &image.sha256))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_an_alias_changes_its_content_url_and_alias_is_never_cached() {
        let old = crate::domain::sha256_hex(b"old photo");
        let new = crate::domain::sha256_hex(b"edited photo");
        let path = "/api/artist-links/artist/photo";
        let a = redirect(path, &old);
        let b = redirect(path, &new);
        assert_ne!(a.headers()[header::LOCATION], b.headers()[header::LOCATION]);
        assert_eq!(
            a.headers()[header::LOCATION],
            format!("{path}?revision={old}")
        );
        assert_eq!(a.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(b.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(b.status(), StatusCode::TEMPORARY_REDIRECT);
        assert!(valid_digest(&old));
        for bad in ["", "../", "*", &"A".repeat(64)] {
            assert!(!valid_digest(bad));
        }
    }
}
