use crate::error::{Error, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct ObjectMeta {
    pub size: i64,
    pub content_type: String,
    pub nonce: String,
    pub etag: String,
}
/// One stored part of a multipart upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedPart {
    pub number: i32,
    pub size: i64,
    pub etag: String,
}
#[derive(Serialize)]
pub struct UploadGrant {
    pub url: String,
    pub method: &'static str,
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}
/// What an archive write uploads: bytes in memory, or a file streamed from disk.
pub enum ArchiveBody<'a> {
    Bytes(&'a [u8]),
    File(&'a std::path::Path),
}
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn presign_put(
        &self,
        key: &str,
        size: i64,
        mime: &str,
        nonce: &str,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant>;
    /// Start a multipart upload (resumable large masters) at a quarantine
    /// key, with the content type and nonce `head` checks at completion.
    /// Stores without multipart refuse, and Studio falls back to one PUT.
    async fn create_multipart(&self, _key: &str, _mime: &str, _nonce: &str) -> Result<String> {
        Err(Error::PolicyGate("UPLOAD_MULTIPART_UNSUPPORTED"))
    }
    /// A signed PUT for one part of exactly `size` bytes.
    async fn presign_part(
        &self,
        _key: &str,
        _upload_id: &str,
        _part: i32,
        _size: i64,
        _expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        Err(Error::PolicyGate("UPLOAD_MULTIPART_UNSUPPORTED"))
    }
    /// Parts the store holds for an open multipart upload (the resume point).
    async fn list_parts(&self, _key: &str, _upload_id: &str) -> Result<Vec<UploadedPart>> {
        Err(Error::PolicyGate("UPLOAD_MULTIPART_UNSUPPORTED"))
    }
    async fn complete_multipart(
        &self,
        _key: &str,
        _upload_id: &str,
        _parts: &[UploadedPart],
    ) -> Result<()> {
        Err(Error::PolicyGate("UPLOAD_MULTIPART_UNSUPPORTED"))
    }
    /// Drop an unfinished multipart upload; an unknown one is success.
    async fn abort_multipart(&self, _key: &str, _upload_id: &str) -> Result<()> {
        Ok(())
    }
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>>;
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()>;
    /// Whether [`ObjectStore::freeze`]'s copy carries the source ETag (false
    /// on OCI, where the copy is matched on size, type and nonce instead).
    fn copy_keeps_etag(&self) -> bool {
        true
    }
    /// Delete an object; a missing object is success. Used to drop the
    /// quarantine copy once an upload is completed or cancelled. The default
    /// is a no-op for stores without deletion (test doubles).
    async fn delete(&self, _key: &str) -> Result<()> {
        Ok(())
    }
    /// Store a local file under `key` with the server's own credentials
    /// (a master the server derived, e.g. FLAC transcoded from ALAC). The
    /// body streams from disk; the byte count is signed like a user upload.
    async fn put_file(
        &self,
        _key: &str,
        _path: &std::path::Path,
        _mime: &str,
        _nonce: &str,
    ) -> Result<()> {
        Err(Error::Storage)
    }
    /// Write an archive object (original-document archive, integrity
    /// anchors, database backups) with the server's credentials. With
    /// `retain_until`, an S3 Object Lock bucket keeps it in COMPLIANCE mode
    /// until then (nobody, the root account included, can delete or replace
    /// it); R2 and other buckets enforce retention with bucket lock rules.
    async fn put_archive(
        &self,
        _key: &str,
        _body: ArchiveBody<'_>,
        _mime: &str,
        _retain_until: Option<DateTime<Utc>>,
    ) -> Result<()> {
        Err(Error::Storage)
    }
    /// Download full object bytes. Small objects only (artwork, test
    /// fixtures); audio goes through [`ObjectStore::download_to`] or
    /// [`ObjectStore::digest`], which never hold the whole object in memory.
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    /// First `n` bytes of an object (content sniffing at upload completion)
    /// without transferring the rest. The default reads the whole object;
    /// the S3 store asks for a byte range.
    async fn read_prefix(&self, key: &str, n: usize) -> Result<Vec<u8>> {
        let mut b = self.get(key).await?;
        b.truncate(n);
        Ok(b)
    }
    /// Stream an object into `dest`, hashing it on the way. Fails with
    /// [`Error::PolicyGate`]`("OBJECT_TOO_LARGE")` once more than `max_bytes`
    /// arrive, so a lying HEAD or a replaced object cannot fill the disk.
    /// The default implementation buffers through [`ObjectStore::get`]; the
    /// S3 store overrides it with a constant-memory stream.
    async fn download_to(
        &self,
        key: &str,
        dest: &std::path::Path,
        max_bytes: u64,
    ) -> Result<ObjectDigest> {
        let bytes = self.get(key).await?;
        if bytes.len() as u64 > max_bytes {
            return Err(Error::PolicyGate("OBJECT_TOO_LARGE"));
        }
        tokio::fs::write(dest, &bytes)
            .await
            .map_err(|_| Error::Internal)?;
        Ok(ObjectDigest::of(&bytes))
    }
    /// [`ObjectStore::download_to`] of exactly the version a HEAD saw: the
    /// GET carries `If-Match: etag`, so any other object answers 412
    /// ([`Error::Conflict`]). Upload completion reads the quarantine object
    /// this way instead of copying it first: the signed PUT is single use, so
    /// the object cannot change, and If-Match proves it.
    async fn download_version_to(
        &self,
        key: &str,
        dest: &std::path::Path,
        max_bytes: u64,
        _etag: &str,
    ) -> Result<ObjectDigest> {
        self.download_to(key, dest, max_bytes).await
    }
    /// SHA-256, size and leading bytes of an object without keeping it on
    /// disk or in memory (upload completion). Same size cap as `download_to`.
    async fn digest(&self, key: &str, max_bytes: u64) -> Result<ObjectDigest> {
        let bytes = self.get(key).await?;
        if bytes.len() as u64 > max_bytes {
            return Err(Error::PolicyGate("OBJECT_TOO_LARGE"));
        }
        Ok(ObjectDigest::of(&bytes))
    }
}
/// Streaming content digest of one stored object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectDigest {
    pub size: u64,
    /// Lowercase hex SHA-256 of the full object.
    pub sha256: String,
    /// First bytes of the object (up to [`HEAD_SNIFF_BYTES`]) for content sniffing.
    pub head: Vec<u8>,
}
/// Leading bytes kept for magic-number content sniffing.
pub const HEAD_SNIFF_BYTES: usize = 64;
/// Buffered reads are only for artwork/documents. Audio must stream.
const MAX_BUFFERED_BYTES: u64 = 20 * 1024 * 1024;
impl ObjectDigest {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            size: bytes.len() as u64,
            sha256: crate::domain::sha256_hex(bytes),
            head: bytes[..bytes.len().min(HEAD_SNIFF_BYTES)].to_vec(),
        }
    }
}
/// Incremental digest builder used by the streaming implementations.
struct DigestBuilder {
    hasher: Sha256,
    size: u64,
    head: Vec<u8>,
    max: u64,
}
impl DigestBuilder {
    fn new(max: u64) -> Self {
        Self {
            hasher: Sha256::new(),
            size: 0,
            head: Vec::with_capacity(HEAD_SNIFF_BYTES),
            max,
        }
    }
    fn update(&mut self, chunk: &[u8]) -> Result<()> {
        if chunk.len() as u64 > self.max.saturating_sub(self.size) {
            return Err(Error::PolicyGate("OBJECT_TOO_LARGE"));
        }
        self.size += chunk.len() as u64;
        if self.head.len() < HEAD_SNIFF_BYTES {
            let take = (HEAD_SNIFF_BYTES - self.head.len()).min(chunk.len());
            self.head.extend_from_slice(&chunk[..take]);
        }
        self.hasher.update(chunk);
        Ok(())
    }
    fn finish(self) -> ObjectDigest {
        ObjectDigest {
            size: self.size,
            sha256: hex::encode(self.hasher.finalize()),
            head: self.head,
        }
    }
}
pub struct DisabledStore;
#[async_trait]
impl ObjectStore for DisabledStore {
    async fn presign_put(
        &self,
        _: &str,
        _: i64,
        _: &str,
        _: &str,
        _: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        Err(Error::Storage)
    }
    async fn head(&self, _: &str) -> Result<Option<ObjectMeta>> {
        Err(Error::Storage)
    }
    async fn freeze(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Err(Error::Storage)
    }
    async fn get(&self, _: &str) -> Result<Vec<u8>> {
        Err(Error::Storage)
    }
}
/// S3 SigV4 query signing uses HMAC-SHA256 from RustCrypto. No credentials are serialized.
/// The S3-compatible service behind an endpoint. Uploads use the same S3
/// calls everywhere (SigV4, path-style `/bucket/key`); these are the
/// differences that matter (docs/STORAGE_PROVIDERS.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Cloudflare R2: `<account>.r2.cloudflarestorage.com`, region `auto`.
    R2,
    /// AWS S3: `s3.<region>.amazonaws.com`.
    Aws,
    /// Oracle Cloud Object Storage (S3 Compatibility API):
    /// `<namespace>.compat.objectstorage.<region>.oraclecloud.com` (or
    /// `.oci.customer-oci.com`); signed with the OCI region id.
    Oci,
    /// MinIO and other S3-compatible stores.
    Other,
}

impl Provider {
    pub fn detect(host: &str) -> Self {
        let host = host.to_ascii_lowercase();
        if host.ends_with(".r2.cloudflarestorage.com") {
            Self::R2
        } else if host.ends_with(".amazonaws.com") {
            Self::Aws
        } else if host.contains(".compat.objectstorage.")
            && (host.ends_with(".oraclecloud.com") || host.ends_with(".oci.customer-oci.com"))
        {
            Self::Oci
        } else {
            Self::Other
        }
    }

    /// The signing region written in the endpoint host, if it has one.
    fn host_region(self, host: &str) -> Option<String> {
        let labels: Vec<&str> = host.split('.').collect();
        let region = match self {
            // <namespace>.compat.objectstorage.<region>.…
            Self::Oci => labels
                .iter()
                .position(|l| *l == "objectstorage")
                .and_then(|i| labels.get(i + 1)),
            // s3.<region>.amazonaws.com, s3.dualstack.<region>.amazonaws.com
            Self::Aws => {
                let before = labels.len().checked_sub(2)?;
                labels[..before]
                    .iter()
                    .rev()
                    .find(|l| **l != "s3" && **l != "dualstack" && !l.starts_with("s3-"))
            }
            Self::R2 | Self::Other => None,
        }?;
        (!region.is_empty()
            && region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
        .then(|| region.to_string())
    }

    /// Whether a server-side copy keeps the source ETag. OCI gives every
    /// object its own ETag, so a copy is matched on size, type and nonce
    /// (and the content is checked in full afterwards anyway).
    pub fn copy_keeps_etag(self) -> bool {
        self != Self::Oci
    }

    /// Whether per-object S3 Object Lock headers are honoured. R2 and OCI
    /// keep originals with bucket-level rules (R2 bucket lock, OCI
    /// retention rules) instead; they would ignore the headers.
    pub fn object_lock_headers(self) -> bool {
        matches!(self, Self::Aws | Self::Other)
    }
}

/// Signing region: an explicit `S3_REGION` (other than `auto`) wins and must
/// match the host for OCI/AWS; otherwise taken from the host. R2 signs `auto`.
fn resolve_region(provider: Provider, host: &str, region: &str) -> anyhow::Result<String> {
    let region = region.trim();
    let from_host = provider.host_region(host);
    match provider {
        Provider::R2 => Ok(if region.is_empty() {
            "auto".into()
        } else {
            region.into()
        }),
        Provider::Oci | Provider::Aws => {
            if region.is_empty() || region == "auto" {
                if let Some(r) = from_host {
                    return Ok(r);
                }
                // s3.amazonaws.com (no region in the host) signs us-east-1.
                anyhow::ensure!(
                    provider == Provider::Aws,
                    "OCI endpoint must be https://<namespace>.compat.objectstorage.<region>.oraclecloud.com"
                );
                return Ok("us-east-1".into());
            }
            if let Some(r) = from_host {
                anyhow::ensure!(
                    r == region,
                    "S3_REGION {region} does not match the endpoint region {r}"
                );
            }
            Ok(region.into())
        }
        Provider::Other => Ok(if region.is_empty() {
            "auto".into()
        } else {
            region.into()
        }),
    }
}

pub struct S3Store {
    endpoint: url::Url,
    bucket: String,
    access: String,
    secret: String,
    region: String,
    provider: Provider,
    client: reqwest::Client,
    /// Object downloads can legitimately take minutes (512 MiB masters), so
    /// they use an idle (read) timeout instead of the 10 s total timeout.
    download_client: reqwest::Client,
}
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn hmac(key: &[u8], s: &str) -> Vec<u8> {
    let mut h = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key");
    h.update(s.as_bytes());
    h.finalize().into_bytes().to_vec()
}
/// Server-side copy into quarantine: R2/S3 answer CopyObject only when the
/// copy is done, which for a 200–500 MiB master takes longer than the 10 s
/// every metadata call gets (100-album test BE-012: completions failed at
/// ~10.2 s with STORAGE_UNAVAILABLE). Only this call gets the longer limit.
const COPY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Log why a storage call failed — operation, error class, elapsed time —
/// and never the (signed) URL or key, then map to [`Error::Storage`].
fn transport_error(op: &'static str, started: std::time::Instant, e: &reqwest::Error) -> Error {
    let kind = if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else if e.is_body() || e.is_decode() {
        "body"
    } else {
        "request"
    };
    tracing::warn!(
        storage_op = op,
        kind,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "storage_call_failed"
    );
    Error::Storage
}
fn status_error(
    op: &'static str,
    started: std::time::Instant,
    status: reqwest::StatusCode,
) -> Error {
    tracing::warn!(
        storage_op = op,
        kind = "status",
        status = status.as_u16(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "storage_call_failed"
    );
    Error::Storage
}
/// Worth one more try: the request never reached the store, or the store
/// said it is temporarily unable. A timeout is not retried (the copy may
/// still be running; the caller's retry reuses the same session).
fn transient_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 500 | 502 | 503 | 504)
}

/// Parts of a `ListPartsResult` (PartNumber, ETag, Size in document order).
fn parse_parts(xml: &str) -> Result<Vec<UploadedPart>> {
    let numbers = crate::transport::xml_values(xml, "PartNumber");
    let etags = crate::transport::xml_values(xml, "ETag");
    let sizes = crate::transport::xml_values(xml, "Size");
    if numbers.len() != etags.len() || numbers.len() != sizes.len() {
        return Err(Error::Storage);
    }
    numbers
        .into_iter()
        .zip(etags)
        .zip(sizes)
        .map(|((n, etag), size)| {
            Ok(UploadedPart {
                number: n.parse().map_err(|_| Error::Storage)?,
                size: size.parse().map_err(|_| Error::Storage)?,
                etag,
            })
        })
        .collect()
}

impl S3Store {
    pub fn new(
        endpoint: &str,
        bucket: String,
        access: String,
        secret: String,
        region: String,
        allow_http: bool,
    ) -> anyhow::Result<Self> {
        let allow_http = crate::external_http::local_http_enabled(allow_http);
        let endpoint = crate::external_http::validate_url(endpoint, allow_http)?;
        anyhow::ensure!(
            endpoint.scheme() == "https" || (allow_http && endpoint.scheme() == "http"),
            "S3 requires HTTPS"
        );
        anyhow::ensure!(
            endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none()
                && endpoint.host_str().is_some()
                && endpoint.path() == "/",
            "invalid S3 endpoint"
        );
        anyhow::ensure!(
            !bucket.is_empty()
                && bucket
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b)),
            "invalid bucket"
        );
        let host = endpoint.host_str().unwrap_or_default().to_string();
        let provider = Provider::detect(&host);
        let region = resolve_region(provider, &host, &region)?;
        Ok(Self {
            endpoint,
            bucket,
            access,
            secret,
            region,
            provider,
            client: crate::external_http::client_builder(allow_http)
                .no_gzip()
                .no_brotli()
                .no_zstd()
                .no_deflate()
                .timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            download_client: crate::external_http::client_builder(allow_http)
                .no_gzip()
                .no_brotli()
                .no_zstd()
                .no_deflate()
                .timeout(std::time::Duration::from_secs(900))
                .connect_timeout(std::time::Duration::from_secs(10))
                .read_timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
    pub fn provider(&self) -> Provider {
        self.provider
    }
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Tries the calls uploads depend on against the real bucket, the way a
    /// browser and the API make them, and reports what the store enforces:
    /// the signed PUT works and is single use (If-None-Match), metadata
    /// comes back, the freeze copy works (and whether it is conditional and
    /// keeps the ETag), and — with `origin` — the CORS preflight a browser
    /// on that origin sends. Leaves nothing behind but `quarantine/` objects
    /// the lifecycle rule removes.
    pub async fn probe(&self, origin: Option<&str>) -> serde_json::Value {
        use serde_json::json;
        let id = uuid::Uuid::new_v4();
        let key = format!("quarantine/probe/{id}");
        let body = b"audeniq storage probe".to_vec();
        let nonce = uuid::Uuid::new_v4().to_string();
        let mime = "application/octet-stream";
        let mut checks = serde_json::Map::new();
        let mut problems: Vec<&str> = Vec::new();
        let mut warnings: Vec<&str> = Vec::new();
        let grant = match self
            .presign_put(
                &key,
                body.len() as i64,
                mime,
                &nonce,
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
        {
            Ok(g) => g,
            Err(_) => {
                return json!({"ok": false, "problems": ["PRESIGN_FAILED"]});
            }
        };
        let put = |extra: Option<&str>| {
            let mut req = self.client.put(&grant.url);
            for (k, v) in &grant.headers {
                // Browsers set Content-Length themselves; so does reqwest.
                if k != "content-length" {
                    req = req.header(k, v);
                }
            }
            if let Some(o) = extra {
                req = req.header("origin", o);
            }
            req.body(body.clone()).send()
        };
        let first = put(origin).await.map(|r| r.status().as_u16()).ok();
        let put_ok = matches!(first, Some(200..=299));
        checks.insert("signed_put".into(), json!(first));
        if !put_ok {
            problems.push("SIGNED_PUT_FAILED");
        }
        let second = put(None).await.map(|r| r.status().as_u16()).ok();
        checks.insert("second_put".into(), json!(second));
        if put_ok && second != Some(412) {
            problems.push("SIGNED_PUT_NOT_SINGLE_USE");
        }
        let meta = self.head(&key).await.ok().flatten();
        let meta_ok = meta.as_ref().is_some_and(|m| {
            m.size == body.len() as i64 && m.content_type == mime && m.nonce == nonce
        });
        checks.insert("metadata".into(), json!(meta_ok));
        if put_ok && !meta_ok {
            problems.push("METADATA_NOT_RETURNED");
        }
        let mut cleanup = vec![key.clone()];
        if let Some(meta) = meta.filter(|_| meta_ok) {
            let wrong = format!("quarantine/probe/{id}-wrong-etag");
            let conditional = self.freeze(&key, &wrong, "\"0000\"").await.is_err();
            cleanup.push(wrong);
            checks.insert("conditional_copy".into(), json!(conditional));
            if !conditional {
                warnings.push("COPY_IGNORES_IF_MATCH");
            }
            let frozen = format!("quarantine/probe/{id}-frozen");
            let copied = self.freeze(&key, &frozen, &meta.etag).await.is_ok();
            cleanup.push(frozen.clone());
            checks.insert("copy".into(), json!(copied));
            let copy = if copied {
                self.head(&frozen).await.ok().flatten()
            } else {
                None
            };
            match copy {
                None => problems.push("COPY_FAILED"),
                Some(c) => {
                    let keeps = c.etag == meta.etag;
                    checks.insert("copy_keeps_etag".into(), json!(keeps));
                    if c.size != meta.size || c.content_type != mime || c.nonce != nonce {
                        problems.push("COPY_DROPS_METADATA");
                    } else if self.provider.copy_keeps_etag() && !keeps {
                        problems.push("COPY_CHANGES_ETAG");
                    }
                }
            }
        }
        if let Some(origin) = origin {
            let wanted = ["content-type", "if-none-match", "x-amz-meta-upload-nonce"];
            let r = self
                .client
                .request(reqwest::Method::OPTIONS, &grant.url)
                .header("origin", origin)
                .header("access-control-request-method", "PUT")
                .header("access-control-request-headers", wanted.join(","))
                .send()
                .await
                .ok();
            let header = |name: &str| {
                r.as_ref()
                    .and_then(|r| r.headers().get(name))
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.to_ascii_lowercase())
                    .unwrap_or_default()
            };
            let allow_origin = header("access-control-allow-origin");
            let methods = header("access-control-allow-methods");
            let headers = header("access-control-allow-headers");
            let ok = r.as_ref().is_some_and(|r| r.status().is_success())
                && (allow_origin == origin.to_ascii_lowercase() || allow_origin == "*")
                && (methods.is_empty() || methods.contains("put") || methods == "*")
                && (headers == "*" || wanted.iter().all(|h| headers.contains(h)));
            checks.insert(
                "cors_preflight".into(),
                json!({"ok": ok, "status": r.as_ref().map(|r| r.status().as_u16()),
                       "allow_origin": allow_origin, "allow_methods": methods, "allow_headers": headers}),
            );
            if !ok {
                problems.push("CORS_PREFLIGHT_REFUSED");
            }
        } else {
            warnings.push("CORS_NOT_CHECKED");
        }
        for k in &cleanup {
            let _ = self.delete(k).await;
        }
        json!({
            "ok": problems.is_empty(),
            "provider": self.provider, "region": self.region, "bucket": self.bucket,
            "checks": checks, "problems": problems, "warnings": warnings,
        })
    }

    /// The archive bucket: a checked write (Content-MD5, plus a one-day
    /// Object Lock where the provider takes the headers) reads back intact,
    /// and a delete is refused (bucket lock / retention rule / Object Lock).
    /// The probe object stays for the rule's retention period: it is tiny.
    pub async fn probe_archive(&self) -> serde_json::Value {
        use serde_json::json;
        let key = format!("probe/{}.txt", uuid::Uuid::new_v4());
        let body = format!("audeniq archive probe {}", Utc::now().to_rfc3339());
        let lock = (std::env::var("ARCHIVE_OBJECT_LOCK").as_deref() == Ok("true"))
            .then(|| Utc::now() + chrono::Duration::days(1));
        let mut problems: Vec<&str> = Vec::new();
        let put = self
            .put_archive(
                &key,
                ArchiveBody::Bytes(body.as_bytes()),
                "text/plain",
                lock,
            )
            .await
            .is_ok();
        if !put {
            problems.push("ARCHIVE_PUT_FAILED");
        }
        let read_back = put && self.get(&key).await.is_ok_and(|b| b == body.as_bytes());
        if put && !read_back {
            problems.push("ARCHIVE_READ_BACK_FAILED");
        }
        // A WORM bucket refuses the delete, or keeps the object anyway.
        let deleted = put && self.delete(&key).await.is_ok();
        let still_there = put && self.get(&key).await.is_ok();
        let worm = put && (!deleted || still_there);
        if put && !worm {
            problems.push("ARCHIVE_NOT_WORM");
        }
        json!({
            "ok": problems.is_empty(),
            "provider": self.provider, "region": self.region, "bucket": self.bucket,
            "object_lock_headers": lock.is_some(),
            "checks": {"put": put, "read_back": read_back, "delete_refused": put && !deleted, "kept_after_delete": still_there},
            "problems": problems,
        })
    }

    fn signed(
        &self,
        method: &str,
        key: &str,
        headers: &BTreeMap<String, String>,
        at: DateTime<Utc>,
        ttl: i64,
    ) -> Result<String> {
        self.signed_query(method, key, &[], headers, at, ttl)
    }

    /// [`S3Store::signed`] with extra query parameters that are part of the
    /// signature (multipart: `uploads`, `uploadId`, `partNumber`).
    fn signed_query(
        &self,
        method: &str,
        key: &str,
        extra: &[(&str, &str)],
        headers: &BTreeMap<String, String>,
        at: DateTime<Utc>,
        ttl: i64,
    ) -> Result<String> {
        if !(1..=3600).contains(&ttl)
            || key.is_empty()
            || key.len() > 1024
            || key.bytes().any(|b| b.is_ascii_control() || b == b'\\')
            || key
                .split('/')
                .any(|p| p.is_empty() || p == ".." || p == ".")
            || headers.iter().any(|(k, v)| {
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).is_err()
                    || reqwest::header::HeaderValue::from_str(v).is_err()
                    || k != &k.to_ascii_lowercase()
            })
        {
            return Err(Error::Invalid);
        }
        let path = format!(
            "/{}/{}",
            self.bucket,
            key.split('/').map(enc).collect::<Vec<_>>().join("/")
        );
        let host = self.endpoint[url::Position::BeforeHost..url::Position::AfterPort].to_string();
        let mut signed = headers.clone();
        signed.insert("host".into(), host.clone());
        let names = signed.keys().cloned().collect::<Vec<_>>().join(";");
        let canonical_headers = signed
            .iter()
            .map(|(k, v)| {
                format!(
                    "{k}:{}\n",
                    v.split_whitespace().collect::<Vec<_>>().join(" ")
                )
            })
            .collect::<String>();
        let date = at.format("%Y%m%d").to_string();
        let datetime = at.format("%Y%m%dT%H%M%SZ").to_string();
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let mut query = BTreeMap::from([
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            ("X-Amz-Credential", format!("{}/{}", self.access, scope)),
            ("X-Amz-Date", datetime.clone()),
            ("X-Amz-Expires", ttl.to_string()),
            ("X-Amz-SignedHeaders", names.clone()),
        ]);
        for (k, v) in extra {
            if query.insert(k, (*v).to_string()).is_some() {
                return Err(Error::Invalid);
            }
        }
        let query = query
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect::<Vec<_>>()
            .join("&");
        let canonical =
            format!("{method}\n{path}\n{query}\n{canonical_headers}\n{names}\nUNSIGNED-PAYLOAD");
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
            crate::domain::sha256_hex(canonical)
        );
        let k_date = hmac(format!("AWS4{}", self.secret).as_bytes(), &date);
        let k_region = hmac(&k_date, &self.region);
        let k_service = hmac(&k_region, "s3");
        let key = hmac(&k_service, "aws4_request");
        Ok(format!(
            "{}://{host}{path}?{query}&X-Amz-Signature={}",
            self.endpoint.scheme(),
            hex::encode(hmac(&key, &to_sign))
        ))
    }
}
#[async_trait]
impl ObjectStore for S3Store {
    fn copy_keeps_etag(&self) -> bool {
        self.provider.copy_keeps_etag()
    }
    async fn presign_put(
        &self,
        key: &str,
        size: i64,
        mime: &str,
        nonce: &str,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        // The user gets only this URL, never the key: it can PUT exactly one
        // object (this key, type, nonce and byte count) until it expires.
        // Content-Length is signed so the store itself refuses a body of any
        // other size; browsers send it for a Blob body on their own (script
        // may not set it, so Studio skips it when copying the headers).
        // Completion still re-checks size, type, nonce and content.
        // Single use: If-None-Match: * is signed too, so R2 only accepts the
        // PUT while the key is empty. After one successful upload (or a
        // leaked copy of the URL racing it) every further PUT gets 412; the
        // quarantine object is kept until the bucket lifecycle rule expires
        // it, long after the 10-minute grant.
        if !(1..=crate::uploads::MAX_AUDIO_BYTES).contains(&size) || !key.starts_with("quarantine/")
        {
            return Err(Error::Invalid);
        }
        let headers = BTreeMap::from([
            ("content-length".into(), size.to_string()),
            ("content-type".into(), mime.into()),
            ("if-none-match".into(), "*".into()),
            ("x-amz-meta-upload-nonce".into(), nonce.into()),
        ]);
        let now = Utc::now();
        let ttl = (expires - now).num_seconds();
        let url = self.signed("PUT", key, &headers, now, ttl)?;
        Ok(UploadGrant {
            url,
            method: "PUT",
            headers,
            expires_at: expires,
        })
    }
    async fn create_multipart(&self, key: &str, mime: &str, nonce: &str) -> Result<String> {
        if !key.starts_with("quarantine/") {
            return Err(Error::Invalid);
        }
        let headers = BTreeMap::from([
            ("content-type".into(), mime.into()),
            ("x-amz-meta-upload-nonce".into(), nonce.into()),
        ]);
        let started = std::time::Instant::now();
        let url = self.signed_query("POST", key, &[("uploads", "")], &headers, Utc::now(), 60)?;
        let mut req = self.client.post(url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let r = req
            .send()
            .await
            .map_err(|e| transport_error("multipart_create", started, &e))?;
        if !r.status().is_success() {
            return Err(status_error("multipart_create", started, r.status()));
        }
        let body = r.text().await.map_err(|_| Error::Storage)?;
        crate::transport::xml_values(&body, "UploadId")
            .into_iter()
            .next()
            .filter(|id| !id.is_empty() && id.len() <= 1024)
            .ok_or(Error::Storage)
    }
    async fn presign_part(
        &self,
        key: &str,
        upload_id: &str,
        part: i32,
        size: i64,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        // Content-Length is signed: the store refuses a part of any other
        // size, so the parts can only add up to the declared master size.
        if !(1..=10_000).contains(&part)
            || !(1..=crate::uploads::MAX_AUDIO_BYTES).contains(&size)
            || !key.starts_with("quarantine/")
            || upload_id.is_empty()
        {
            return Err(Error::Invalid);
        }
        let headers = BTreeMap::from([("content-length".into(), size.to_string())]);
        let now = Utc::now();
        let part = part.to_string();
        let url = self.signed_query(
            "PUT",
            key,
            &[("partNumber", &part), ("uploadId", upload_id)],
            &headers,
            now,
            (expires - now).num_seconds(),
        )?;
        Ok(UploadGrant {
            url,
            method: "PUT",
            headers,
            expires_at: expires,
        })
    }
    async fn list_parts(&self, key: &str, upload_id: &str) -> Result<Vec<UploadedPart>> {
        let started = std::time::Instant::now();
        let url = self.signed_query(
            "GET",
            key,
            &[("uploadId", upload_id)],
            &BTreeMap::new(),
            Utc::now(),
            60,
        )?;
        let r = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| transport_error("multipart_list", started, &e))?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::PolicyGate("UPLOAD_MULTIPART_GONE"));
        }
        if !r.status().is_success() {
            return Err(status_error("multipart_list", started, r.status()));
        }
        let body = r.text().await.map_err(|_| Error::Storage)?;
        parse_parts(&body)
    }
    async fn complete_multipart(
        &self,
        key: &str,
        upload_id: &str,
        parts: &[UploadedPart],
    ) -> Result<()> {
        let mut xml = String::from("<CompleteMultipartUpload>");
        for p in parts {
            xml.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                p.number,
                quick_xml::escape::escape(&p.etag)
            ));
        }
        xml.push_str("</CompleteMultipartUpload>");
        let headers = BTreeMap::from([("content-type".into(), "application/xml".into())]);
        let started = std::time::Instant::now();
        let url = self.signed_query(
            "POST",
            key,
            &[("uploadId", upload_id)],
            &headers,
            Utc::now(),
            300,
        )?;
        let r = self
            .client
            .post(url)
            .timeout(COPY_TIMEOUT)
            .header("content-type", "application/xml")
            .body(xml)
            .send()
            .await
            .map_err(|e| transport_error("multipart_complete", started, &e))?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::PolicyGate("UPLOAD_MULTIPART_GONE"));
        }
        if !r.status().is_success() {
            return Err(status_error("multipart_complete", started, r.status()));
        }
        // S3 can answer 200 and still report a failure in the body.
        let body = r.text().await.map_err(|_| Error::Storage)?;
        if body.contains("<Error>") {
            return Err(status_error(
                "multipart_complete",
                started,
                reqwest::StatusCode::BAD_GATEWAY,
            ));
        }
        Ok(())
    }
    async fn abort_multipart(&self, key: &str, upload_id: &str) -> Result<()> {
        let url = self.signed_query(
            "DELETE",
            key,
            &[("uploadId", upload_id)],
            &BTreeMap::new(),
            Utc::now(),
            60,
        )?;
        let r = self
            .client
            .delete(url)
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(Error::Storage)
        }
    }
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>> {
        let started = std::time::Instant::now();
        let url = self.signed("HEAD", key, &BTreeMap::new(), Utc::now(), 60)?;
        let r = self
            .client
            .head(url)
            .send()
            .await
            .map_err(|e| transport_error("head", started, &e))?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(status_error("head", started, r.status()));
        }
        let h = r.headers();
        let get = |k: &str| {
            h.get(k)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
                .ok_or(Error::Storage)
        };
        Ok(Some(ObjectMeta {
            size: get("content-length")?.parse().map_err(|_| Error::Storage)?,
            content_type: get("content-type")?,
            nonce: get("x-amz-meta-upload-nonce")?,
            etag: get("etag")?,
        }))
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        let headers = BTreeMap::from([
            (
                "x-amz-copy-source".into(),
                format!(
                    "/{}/{}",
                    self.bucket,
                    source.split('/').map(enc).collect::<Vec<_>>().join("/")
                ),
            ),
            ("x-amz-copy-source-if-match".into(), etag.into()),
            ("x-amz-metadata-directive".into(), "COPY".into()),
        ]);
        // Copying to the same target with the same source ETag condition is
        // idempotent, so one retry after a transient failure is safe.
        for attempt in 0..2 {
            let started = std::time::Instant::now();
            let url = self.signed("PUT", target, &headers, Utc::now(), 60)?;
            let mut req = self.client.put(url).timeout(COPY_TIMEOUT);
            for (k, v) in &headers {
                req = req.header(k, v);
            }
            let r = match req.send().await {
                Ok(r) => r,
                Err(e) if attempt == 0 && e.is_connect() => {
                    transport_error("copy", started, &e);
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
                Err(e) => return Err(transport_error("copy", started, &e)),
            };
            let status = r.status();
            if !status.is_success() {
                let err = status_error("copy", started, status);
                if attempt == 0 && transient_status(status) {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
                return Err(err);
            }
            // CopyObject can return HTTP 200 with an embedded XML error. Confirm object separately too.
            let body = r
                .text()
                .await
                .map_err(|e| transport_error("copy", started, &e))?;
            if body.contains("<Error>") || !body.contains("CopyObjectResult") {
                let retry = attempt == 0
                    && (body.contains("<Code>InternalError</Code>")
                        || body.contains("<Code>ServiceUnavailable</Code>")
                        || body.contains("<Code>SlowDown</Code>"));
                tracing::warn!(
                    storage_op = "copy",
                    kind = "embedded_error",
                    retry,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "storage_call_failed"
                );
                if retry {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
                return Err(Error::Storage);
            }
            return Ok(());
        }
        Err(Error::Storage)
    }
    async fn delete(&self, key: &str) -> Result<()> {
        let url = self.signed("DELETE", key, &BTreeMap::new(), Utc::now(), 60)?;
        let r = self
            .client
            .delete(url)
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(Error::Storage)
        }
    }
    async fn put_file(
        &self,
        key: &str,
        path: &std::path::Path,
        mime: &str,
        nonce: &str,
    ) -> Result<()> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| Error::Internal)?;
        let size = file.metadata().await.map_err(|_| Error::Internal)?.len();
        let headers = BTreeMap::from([
            ("content-length".into(), size.to_string()),
            ("content-type".into(), mime.into()),
            ("x-amz-meta-upload-nonce".into(), nonce.into()),
        ]);
        let started = std::time::Instant::now();
        let url = self.signed("PUT", key, &headers, Utc::now(), 900)?;
        let mut req = self.download_client.put(url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let r = req
            .body(reqwest::Body::from(file))
            .send()
            .await
            .map_err(|e| transport_error("put", started, &e))?;
        if !r.status().is_success() {
            return Err(status_error("put", started, r.status()));
        }
        Ok(())
    }
    async fn put_archive(
        &self,
        key: &str,
        body: ArchiveBody<'_>,
        mime: &str,
        retain_until: Option<DateTime<Utc>>,
    ) -> Result<()> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use md5::Digest as _;
        // Object Lock writes require Content-MD5; it also guards the transfer.
        let (size, md5) = match &body {
            ArchiveBody::Bytes(b) => (b.len() as u64, md5::Md5::digest(b).to_vec()),
            ArchiveBody::File(path) => {
                use tokio::io::AsyncReadExt;
                let mut f = tokio::fs::File::open(path)
                    .await
                    .map_err(|_| Error::Internal)?;
                let mut h = md5::Md5::new();
                let mut buf = vec![0u8; 1 << 20];
                let mut n = 0u64;
                loop {
                    let read = f.read(&mut buf).await.map_err(|_| Error::Internal)?;
                    if read == 0 {
                        break;
                    }
                    h.update(&buf[..read]);
                    n += read as u64;
                }
                (n, h.finalize().to_vec())
            }
        };
        let mut headers = BTreeMap::from([
            ("content-length".into(), size.to_string()),
            ("content-type".into(), mime.into()),
            ("content-md5".into(), STANDARD.encode(&md5)),
        ]);
        if let Some(until) = retain_until {
            // R2/OCI would silently ignore these: refuse instead of pretending.
            if !self.provider.object_lock_headers() {
                return Err(Error::PolicyGate("OBJECT_LOCK_UNSUPPORTED"));
            }
            headers.insert("x-amz-object-lock-mode".into(), "COMPLIANCE".into());
            headers.insert(
                "x-amz-object-lock-retain-until-date".into(),
                until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            );
        }
        let started = std::time::Instant::now();
        let url = self.signed("PUT", key, &headers, Utc::now(), 3600)?;
        let mut req = self.download_client.put(url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let body = match body {
            ArchiveBody::Bytes(b) => reqwest::Body::from(b.to_vec()),
            ArchiveBody::File(path) => reqwest::Body::from(
                tokio::fs::File::open(path)
                    .await
                    .map_err(|_| Error::Internal)?,
            ),
        };
        let r = req
            .body(body)
            .send()
            .await
            .map_err(|e| transport_error("put", started, &e))?;
        if !r.status().is_success() {
            return Err(status_error("put", started, r.status()));
        }
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let mut r = self.open_download(key, MAX_BUFFERED_BYTES, None).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|_| Error::Storage)? {
            if chunk.len() as u64 > MAX_BUFFERED_BYTES - bytes.len() as u64 {
                return Err(Error::PolicyGate("OBJECT_TOO_LARGE"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    async fn read_prefix(&self, key: &str, n: usize) -> Result<Vec<u8>> {
        if n == 0 {
            return Ok(Vec::new());
        }
        // Range is not signed (it need not be): the URL still grants only a
        // GET of this key for 60 s.
        let url = self.signed("GET", key, &BTreeMap::new(), Utc::now(), 60)?;
        let mut r = self
            .client
            .get(url)
            .header("range", format!("bytes=0-{}", n - 1))
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        if !matches!(
            r.status(),
            reqwest::StatusCode::OK | reqwest::StatusCode::PARTIAL_CONTENT
        ) {
            return Err(Error::Storage);
        }
        let mut bytes = Vec::with_capacity(n);
        while bytes.len() < n {
            match r.chunk().await.map_err(|_| Error::Storage)? {
                Some(chunk) => {
                    let take = (n - bytes.len()).min(chunk.len());
                    bytes.extend_from_slice(&chunk[..take]);
                }
                None => break,
            }
        }
        Ok(bytes)
    }
    async fn download_to(
        &self,
        key: &str,
        dest: &std::path::Path,
        max_bytes: u64,
    ) -> Result<ObjectDigest> {
        self.download_file(key, dest, max_bytes, None).await
    }
    async fn download_version_to(
        &self,
        key: &str,
        dest: &std::path::Path,
        max_bytes: u64,
        etag: &str,
    ) -> Result<ObjectDigest> {
        self.download_file(key, dest, max_bytes, Some(etag)).await
    }
    async fn digest(&self, key: &str, max_bytes: u64) -> Result<ObjectDigest> {
        let mut r = self.open_download(key, max_bytes, None).await?;
        let mut d = DigestBuilder::new(max_bytes);
        while let Some(chunk) = r.chunk().await.map_err(|_| Error::Storage)? {
            d.update(&chunk)?;
        }
        Ok(d.finish())
    }
}
impl S3Store {
    async fn download_file(
        &self,
        key: &str,
        dest: &std::path::Path,
        max_bytes: u64,
        if_match: Option<&str>,
    ) -> Result<ObjectDigest> {
        use tokio::io::AsyncWriteExt;
        let started = std::time::Instant::now();
        let mut r = self.open_download(key, max_bytes, if_match).await?;
        let file = tokio::fs::File::create(dest)
            .await
            .map_err(|_| Error::Internal)?;
        // Coalesce small network chunks instead of dispatching a blocking
        // filesystem write for every HTTP frame.
        let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, file);
        let mut d = DigestBuilder::new(max_bytes);
        while let Some(chunk) = r
            .chunk()
            .await
            .map_err(|e| transport_error("get_body", started, &e))?
        {
            d.update(&chunk)?;
            file.write_all(&chunk).await.map_err(|_| Error::Internal)?;
        }
        file.flush().await.map_err(|_| Error::Internal)?;
        Ok(d.finish())
    }
    async fn open_download(
        &self,
        key: &str,
        max_bytes: u64,
        if_match: Option<&str>,
    ) -> Result<reqwest::Response> {
        let started = std::time::Instant::now();
        let headers = if_match
            .map(|etag| BTreeMap::from([("if-match".to_string(), etag.to_string())]))
            .unwrap_or_default();
        let url = self.signed("GET", key, &headers, Utc::now(), 900)?;
        let mut req = self.download_client.get(url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let r = req
            .send()
            .await
            .map_err(|e| transport_error("get", started, &e))?;
        if r.status() == reqwest::StatusCode::PRECONDITION_FAILED {
            status_error("get", started, r.status());
            return Err(Error::Conflict);
        }
        if r.status() != reqwest::StatusCode::OK {
            return Err(status_error("get", started, r.status()));
        }
        if r.content_length().is_some_and(|size| size > max_bytes) {
            return Err(Error::PolicyGate("OBJECT_TOO_LARGE"));
        }
        Ok(r)
    }
}
/// The original-document archive bucket (S3 or R2): a separate bucket and
/// separate credentials from the working store, ideally another provider or
/// region, with object lock / bucket lock retention. `None` when not set up.
pub fn archive_store_from_env(
    allow_http: bool,
) -> anyhow::Result<Option<std::sync::Arc<dyn ObjectStore>>> {
    let Some(bucket) = std::env::var("ARCHIVE_S3_BUCKET")
        .ok()
        .filter(|b| !b.trim().is_empty())
    else {
        return Ok(None);
    };
    let store = archive_s3_from_env(bucket, allow_http)?;
    anyhow::ensure!(
        std::env::var("ARCHIVE_OBJECT_LOCK").as_deref() != Ok("true")
            || store.provider().object_lock_headers(),
        "ARCHIVE_OBJECT_LOCK=true needs AWS S3 Object Lock; {:?} keeps originals with bucket rules (R2 bucket lock, OCI retention rules) — set ARCHIVE_OBJECT_LOCK=false",
        store.provider()
    );
    Ok(Some(std::sync::Arc::new(store)))
}
fn archive_s3_from_env(bucket: String, allow_http: bool) -> anyhow::Result<S3Store> {
    S3Store::new(
        &std::env::var("ARCHIVE_S3_ENDPOINT")?,
        bucket,
        std::env::var("ARCHIVE_S3_ACCESS_KEY_ID")?,
        std::env::var("ARCHIVE_S3_SECRET_ACCESS_KEY")?,
        std::env::var("ARCHIVE_S3_REGION").unwrap_or_else(|_| "auto".into()),
        allow_http,
    )
}
/// The working bucket (`S3_*`) or, with `archive`, the archive bucket
/// (`ARCHIVE_S3_*`), for `audeniq-admin storage probe`.
pub fn s3_from_env(archive: bool, allow_http: bool) -> anyhow::Result<S3Store> {
    if archive {
        let bucket = std::env::var("ARCHIVE_S3_BUCKET")
            .ok()
            .filter(|b| !b.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("ARCHIVE_S3_BUCKET is not set"))?;
        return archive_s3_from_env(bucket, allow_http);
    }
    S3Store::new(
        &std::env::var("S3_ENDPOINT")?,
        std::env::var("S3_BUCKET")?,
        std::env::var("S3_ACCESS_KEY_ID")?,
        std::env::var("S3_SECRET_ACCESS_KEY")?,
        std::env::var("S3_REGION").unwrap_or_else(|_| "auto".into()),
        allow_http,
    )
}
/// Build the object store from the same env convention as the API binary.
/// Returns an error when STORAGE_ENABLED=true but S3_* vars are missing/invalid.
pub fn store_from_env(allow_http: bool) -> anyhow::Result<std::sync::Arc<dyn ObjectStore>> {
    if std::env::var("STORAGE_ENABLED").as_deref() == Ok("true") {
        Ok(std::sync::Arc::new(S3Store::new(
            &std::env::var("S3_ENDPOINT")?,
            std::env::var("S3_BUCKET")?,
            std::env::var("S3_ACCESS_KEY_ID")?,
            std::env::var("S3_SECRET_ACCESS_KEY")?,
            std::env::var("S3_REGION").unwrap_or_else(|_| "auto".into()),
            allow_http,
        )?))
    } else {
        Ok(std::sync::Arc::new(DisabledStore))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_store(endpoint: &str) -> S3Store {
        S3Store::new(
            endpoint,
            "test".into(),
            "access".into(),
            "secret".into(),
            "auto".into(),
            true,
        )
        .unwrap()
    }

    #[test]
    fn rejects_ambiguous_keys_headers_and_endpoint_fragments() {
        let s = test_store("https://account.r2.cloudflarestorage.com");
        for key in ["", "../a", "a/./b", "a//b", "/a", "a/", "a\\b", "a\nb"] {
            assert!(
                s.signed("GET", key, &BTreeMap::new(), Utc::now(), 60)
                    .is_err(),
                "{key:?}"
            );
        }
        let h = BTreeMap::from([("content-type".into(), "audio/wav\r\nx-injected: yes".into())]);
        assert!(
            s.signed("PUT", "quarantine/test", &h, Utc::now(), 60)
                .is_err()
        );
        assert!(
            S3Store::new(
                "https://example.com/#fragment",
                "test".into(),
                "a".into(),
                "b".into(),
                "auto".into(),
                false
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn client_grants_cannot_write_registered_objects() {
        let s = test_store("https://account.r2.cloudflarestorage.com");
        assert!(
            s.presign_put(
                "registered/org/asset",
                16,
                "audio/wav",
                "nonce",
                Utc::now() + chrono::Duration::minutes(10)
            )
            .await
            .is_err()
        );
    }

    async fn mock_response(response: &'static str) -> S3Store {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await;
            let _ = stream.write_all(response.as_bytes()).await;
        });
        test_store(&endpoint)
    }

    /// Serve `responses` to successive connections, each after its delay;
    /// returns the store and the number of requests that arrived.
    async fn mock_sequence(
        responses: Vec<(u64, &'static str)>,
    ) -> (S3Store, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = seen.clone();
        tokio::spawn(async move {
            for (delay, response) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).await;
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (test_store(&endpoint), seen)
    }

    const COPIED: &str = "HTTP/1.1 200 OK\r\nContent-Length: 39\r\nConnection: close\r\n\r\n<CopyObjectResult></CopyObjectResult>\n\n";
    const UNAVAILABLE: &str =
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const DENIED: &str = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    /// BE-012: a server-side copy of a large master answers after more than
    /// the 10 s metadata timeout; it must still succeed.
    #[tokio::test]
    async fn quarantine_copy_outlasts_the_metadata_timeout() {
        let (s, seen) = mock_sequence(vec![(11_000, COPIED)]).await;
        s.freeze("quarantine/o/a/n", "quarantine/o/a/frozen", "\"e\"")
            .await
            .unwrap();
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn quarantine_copy_retries_once_on_transient_errors_only() {
        let (s, seen) = mock_sequence(vec![(0, UNAVAILABLE), (0, COPIED)]).await;
        s.freeze("quarantine/o/a/n", "quarantine/o/a/frozen", "\"e\"")
            .await
            .unwrap();
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 2);
        let (s, seen) = mock_sequence(vec![(0, UNAVAILABLE), (0, UNAVAILABLE)]).await;
        assert!(matches!(
            s.freeze("quarantine/o/a/n", "quarantine/o/a/frozen", "\"e\"")
                .await,
            Err(Error::Storage)
        ));
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 2);
        // A refusal (a changed source ETag, bad credentials) is final.
        let (s, seen) = mock_sequence(vec![(0, DENIED), (0, COPIED)]).await;
        assert!(matches!(
            s.freeze("quarantine/o/a/n", "quarantine/o/a/frozen", "\"e\"")
                .await,
            Err(Error::Storage)
        ));
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn downloads_reject_oversized_headers_and_chunked_bodies() {
        let s = mock_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 20971521\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(matches!(
            s.get("registered/a").await,
            Err(Error::PolicyGate("OBJECT_TOO_LARGE"))
        ));
        let s = mock_response("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n").await;
        assert!(matches!(
            s.digest("registered/a", 5).await,
            Err(Error::PolicyGate("OBJECT_TOO_LARGE"))
        ));
        let s = mock_response(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
        )
        .await;
        assert!(matches!(
            s.digest("registered/a", 5).await,
            Err(Error::Storage)
        ));
    }

    #[test]
    fn providers_sign_with_their_region() {
        let store = |endpoint: &str, region: &str| {
            S3Store::new(
                endpoint,
                "b".into(),
                "a".into(),
                "s".into(),
                region.into(),
                false,
            )
        };
        let r2 = store("https://acct.r2.cloudflarestorage.com", "auto").unwrap();
        assert_eq!((r2.provider(), r2.region()), (Provider::R2, "auto"));
        let aws = store("https://s3.ap-northeast-2.amazonaws.com", "auto").unwrap();
        assert_eq!(
            (aws.provider(), aws.region()),
            (Provider::Aws, "ap-northeast-2")
        );
        let global = store("https://s3.amazonaws.com", "").unwrap();
        assert_eq!(global.region(), "us-east-1");
        for host in [
            "https://axaxnpcrorw5.compat.objectstorage.ap-chuncheon-1.oraclecloud.com",
            "https://axaxnpcrorw5.compat.objectstorage.ap-chuncheon-1.oci.customer-oci.com",
        ] {
            let oci = store(host, "auto").unwrap();
            assert_eq!(
                (oci.provider(), oci.region()),
                (Provider::Oci, "ap-chuncheon-1")
            );
            assert!(!oci.copy_keeps_etag());
            assert!(!oci.provider().object_lock_headers());
            // An explicit region must agree with the host (else every signature fails).
            assert!(store(host, "ap-seoul-1").is_err());
            assert_eq!(
                store(host, "ap-chuncheon-1").unwrap().region(),
                "ap-chuncheon-1"
            );
        }
        // Signed URLs carry the OCI region in the credential scope.
        let oci = store(
            "https://ns.compat.objectstorage.ap-seoul-1.oraclecloud.com",
            "",
        )
        .unwrap();
        let url = oci
            .signed("PUT", "quarantine/x", &BTreeMap::new(), Utc::now(), 60)
            .unwrap();
        assert!(url.starts_with(
            "https://ns.compat.objectstorage.ap-seoul-1.oraclecloud.com/b/quarantine/x?"
        ));
        assert!(url.contains("%2Fap-seoul-1%2Fs3%2Faws4_request"));
        assert!(Provider::detect("evil.oraclecloud.com.attacker.test") == Provider::Other);
    }

    #[tokio::test]
    async fn object_lock_is_refused_where_it_would_be_ignored() {
        for endpoint in [
            "https://acct.r2.cloudflarestorage.com",
            "https://ns.compat.objectstorage.ap-seoul-1.oraclecloud.com",
        ] {
            let s = test_store(endpoint);
            let lock = Some(Utc::now() + chrono::Duration::days(1));
            assert!(matches!(
                s.put_archive("a/b", ArchiveBody::Bytes(b"x"), "text/plain", lock)
                    .await,
                Err(Error::PolicyGate("OBJECT_LOCK_UNSUPPORTED"))
            ));
        }
    }

    #[test]
    fn digest_limit_is_exact_and_overflow_safe() {
        let mut d = DigestBuilder::new(3);
        d.update(b"abc").unwrap();
        assert!(d.update(b"d").is_err());
        assert_eq!(d.finish(), ObjectDigest::of(b"abc"));
    }
    #[test]
    fn signature_scopes_method_key_headers_and_expiry() {
        let s = S3Store::new(
            "https://account.r2.cloudflarestorage.com",
            "test".into(),
            "example".into(),
            "example-secret".into(),
            "auto".into(),
            false,
        )
        .unwrap();
        let at = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .to_utc();
        let h = BTreeMap::from([("content-length".into(), "100".into())]);
        let a = s.signed("PUT", "a", &h, at, 900).unwrap();
        assert!(a.contains("content-length%3Bhost"));
        assert_ne!(a, s.signed("PUT", "b", &h, at, 900).unwrap());
        assert_ne!(a, s.signed("GET", "a", &h, at, 900).unwrap());
        assert!(s.signed("PUT", "a", &h, at, 0).is_err());
    }
}
