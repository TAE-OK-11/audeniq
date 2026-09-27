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
#[derive(Serialize)]
pub struct UploadGrant {
    pub url: String,
    pub method: &'static str,
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
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
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>>;
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()>;
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
            sha256: hex::encode(Sha256::digest(bytes)),
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
pub struct S3Store {
    endpoint: url::Url,
    bucket: String,
    access: String,
    secret: String,
    region: String,
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
impl S3Store {
    pub fn new(
        endpoint: &str,
        bucket: String,
        access: String,
        secret: String,
        region: String,
        allow_http: bool,
    ) -> anyhow::Result<Self> {
        let endpoint = url::Url::parse(endpoint)?;
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
        Ok(Self {
            endpoint,
            bucket,
            access,
            secret,
            region,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            download_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(900))
                .connect_timeout(std::time::Duration::from_secs(10))
                .read_timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
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
        let query = BTreeMap::from([
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            ("X-Amz-Credential", format!("{}/{}", self.access, scope)),
            ("X-Amz-Date", datetime.clone()),
            ("X-Amz-Expires", ttl.to_string()),
            ("X-Amz-SignedHeaders", names.clone()),
        ]);
        let query = query
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect::<Vec<_>>()
            .join("&");
        let canonical =
            format!("{method}\n{path}\n{query}\n{canonical_headers}\n{names}\nUNSIGNED-PAYLOAD");
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
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
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>> {
        let url = self.signed("HEAD", key, &BTreeMap::new(), Utc::now(), 60)?;
        let r = self
            .client
            .head(url)
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(Error::Storage);
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
        let url = self.signed("PUT", target, &headers, Utc::now(), 60)?;
        let mut req = self.client.put(url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let r = req.send().await.map_err(|_| Error::Storage)?;
        if !r.status().is_success() {
            return Err(Error::Storage);
        }
        // CopyObject can return HTTP 200 with an embedded XML error. Confirm object separately too.
        let body = r.text().await.map_err(|_| Error::Storage)?;
        if body.contains("<Error>") || !body.contains("CopyObjectResult") {
            return Err(Error::Storage);
        }
        Ok(())
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
        let url = self.signed("PUT", key, &headers, Utc::now(), 900)?;
        let mut req = self.download_client.put(url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let r = req
            .body(reqwest::Body::from(file))
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        if !r.status().is_success() {
            return Err(Error::Storage);
        }
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let mut r = self.open_download(key, MAX_BUFFERED_BYTES).await?;
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
        use tokio::io::AsyncWriteExt;
        let mut r = self.open_download(key, max_bytes).await?;
        let file = tokio::fs::File::create(dest)
            .await
            .map_err(|_| Error::Internal)?;
        // Coalesce small network chunks instead of dispatching a blocking
        // filesystem write for every HTTP frame.
        let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, file);
        let mut d = DigestBuilder::new(max_bytes);
        while let Some(chunk) = r.chunk().await.map_err(|_| Error::Storage)? {
            d.update(&chunk)?;
            file.write_all(&chunk).await.map_err(|_| Error::Internal)?;
        }
        file.flush().await.map_err(|_| Error::Internal)?;
        Ok(d.finish())
    }
    async fn digest(&self, key: &str, max_bytes: u64) -> Result<ObjectDigest> {
        let mut r = self.open_download(key, max_bytes).await?;
        let mut d = DigestBuilder::new(max_bytes);
        while let Some(chunk) = r.chunk().await.map_err(|_| Error::Storage)? {
            d.update(&chunk)?;
        }
        Ok(d.finish())
    }
}
impl S3Store {
    async fn open_download(&self, key: &str, max_bytes: u64) -> Result<reqwest::Response> {
        let url = self.signed("GET", key, &BTreeMap::new(), Utc::now(), 900)?;
        let r = self
            .download_client
            .get(url)
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        if r.status() != reqwest::StatusCode::OK {
            return Err(Error::Storage);
        }
        if r.content_length().is_some_and(|size| size > max_bytes) {
            return Err(Error::PolicyGate("OBJECT_TOO_LARGE"));
        }
        Ok(r)
    }
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
