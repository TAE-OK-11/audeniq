//! Cloud KMS and secret-manager clients for the host secret tool
//! (`audeniq-secrets`): AWS (SigV4), GCP (OAuth2 service account / ADC /
//! metadata server) and OCI (API-key request signing), spoken directly over
//! HTTPS. The API and worker never load these clients or any cloud
//! credential; only the operator's host tool does.
//!
//! Errors carry fixed operation messages only. Response bodies, secret
//! values and credentials never reach an error, a log line or stdout.
pub mod aws;
pub mod gcp;
pub mod oci;

use std::time::Duration;
use zeroize::Zeroizing;

/// Largest response body read from any cloud API.
pub const MAX_RESPONSE: usize = 256 * 1024;

#[derive(Debug)]
pub struct CloudError(pub String);

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CloudError {}

pub type Result<T> = std::result::Result<T, CloudError>;

pub fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(CloudError(message.into()))
}

/// HTTPS client used for every cloud API call: TLS 1.2+, certificate
/// verification, no redirects (a redirect could carry a signed request or a
/// secret to another host), bounded time.
pub fn https_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| CloudError("cannot build HTTPS client".into()))
}

/// Plain-HTTP client for link-local instance metadata services only
/// (AWS IMDSv2, GCE metadata); never used for anything else.
pub fn metadata_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| CloudError("cannot build metadata client".into()))
}

/// Send a request and read a bounded body. Non-2xx statuses become `what`
/// failures without echoing the body (it can contain request data).
pub async fn send(request: reqwest::RequestBuilder, what: &str) -> Result<Zeroizing<Vec<u8>>> {
    let mut response = request
        .send()
        .await
        .map_err(|_| CloudError(format!("{what} failed: connection error")))?;
    let status = response.status();
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CloudError(format!("{what} failed: response error")))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE {
            return fail(format!("{what} response exceeds limit"));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return fail(format!(
            "{what} failed (HTTP {}); check IAM, key/secret state and connectivity",
            status.as_u16()
        ));
    }
    Ok(body)
}

pub fn json(body: &[u8], what: &str) -> Result<serde_json::Value> {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v @ serde_json::Value::Object(_)) => Ok(v),
        _ => fail(format!("{what} returned an invalid response")),
    }
}

pub fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub fn unb64(text: &str, what: &str) -> Result<Zeroizing<Vec<u8>>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map(Zeroizing::new)
        .map_err(|_| CloudError(format!("{what} returned invalid base64")))
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    crate::hex::encode(sha2::Sha256::digest(data))
}

/// CRC32C (Castagnoli), as GCP integrity fields use.
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0x82f6_3b78 } else { 0 };
        }
    }
    crc ^ 0xffff_ffff
}

/// An RSA private key from PEM (PKCS#8 "PRIVATE KEY" or PKCS#1
/// "RSA PRIVATE KEY"), for RSASSA-PKCS1-v1_5 SHA-256 signatures.
pub struct RsaSigner(aws_lc_rs::signature::RsaKeyPair);

impl RsaSigner {
    pub fn from_pem(pem: &str) -> Result<Self> {
        use base64::Engine;
        let (label, der) =
            pem_block(pem).ok_or_else(|| CloudError("invalid PEM private key".into()))?;
        let der = Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(der)
                .map_err(|_| CloudError("invalid PEM private key".into()))?,
        );
        let pair = match label.as_str() {
            "PRIVATE KEY" => aws_lc_rs::signature::RsaKeyPair::from_pkcs8(&der),
            "RSA PRIVATE KEY" => aws_lc_rs::signature::RsaKeyPair::from_der(&der),
            _ => return fail("unsupported private key (encrypted keys need decrypting first)"),
        };
        pair.map(Self)
            .map_err(|_| CloudError("invalid RSA private key".into()))
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        let mut signature = vec![0; self.0.public_modulus_len()];
        self.0
            .sign(
                &aws_lc_rs::signature::RSA_PKCS1_SHA256,
                &aws_lc_rs::rand::SystemRandom::new(),
                message,
                &mut signature,
            )
            .map_err(|_| CloudError("RSA signing failed".into()))?;
        Ok(signature)
    }

    /// A fresh RSA-2048 key held only in memory (OCI session tokens).
    pub fn generate() -> Result<Self> {
        aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
            .map(Self)
            .map_err(|_| CloudError("RSA key generation failed".into()))
    }

    /// The public key as X.509 SubjectPublicKeyInfo DER.
    pub fn public_key_spki(&self) -> Result<Vec<u8>> {
        use aws_lc_rs::encoding::AsDer;
        use aws_lc_rs::signature::KeyPair;
        let der: aws_lc_rs::encoding::PublicKeyX509Der = self
            .0
            .public_key()
            .as_der()
            .map_err(|_| CloudError("RSA public key encoding failed".into()))?;
        Ok(der.as_ref().to_vec())
    }

    #[cfg(test)]
    pub fn public_key_der(&self) -> Vec<u8> {
        use aws_lc_rs::signature::KeyPair;
        self.0.public_key().as_ref().to_vec()
    }
}

/// The label and joined base64 body of the first PEM block.
pub(crate) fn pem_block(pem: &str) -> Option<(String, Zeroizing<String>)> {
    let begin = pem.find("-----BEGIN ")?;
    let rest = &pem[begin + 11..];
    let label_end = rest.find("-----")?;
    let label = rest[..label_end].to_string();
    let body_start = label_end + 5;
    let end_marker = format!("-----END {label}-----");
    let end = rest.find(&end_marker)?;
    let body: String = rest[body_start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    Some((label, Zeroizing::new(body)))
}

/// The operator's home directory, for default credential file locations.
pub fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Read a credential/config file (bounded). Credential files hold long-lived
/// keys, so one readable by group/other is refused.
pub fn read_private_file(path: &std::path::Path, what: &str) -> Result<Zeroizing<String>> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).map_err(|_| CloudError(format!("{what} not found")))?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return fail(format!("{what} must be a regular file of at most 64 KiB"));
    }
    if meta.mode() & 0o077 != 0 {
        return fail(format!(
            "{what} must not be readable by group or others (chmod 600)"
        ));
    }
    std::fs::read_to_string(path)
        .map(Zeroizing::new)
        .map_err(|_| CloudError(format!("cannot read {what}")))
}

/// Minimal INI reader for ~/.aws/credentials and ~/.oci/config: the
/// `key = value` pairs of one `[profile]` section.
pub fn ini_section(text: &str, profile: &str) -> Option<std::collections::HashMap<String, String>> {
    let mut found = None;
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(name.trim().to_string());
            if current.as_deref() == Some(profile) {
                found.get_or_insert_with(std::collections::HashMap::new);
            }
            continue;
        }
        if current.as_deref() == Some(profile)
            && let (Some(map), Some((k, v))) = (found.as_mut(), line.split_once('='))
        {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_known_vectors() {
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c(b"123456789"), 0xe306_9283);
    }

    #[test]
    fn ini_reads_only_the_named_profile() {
        let text = "[default]\nkey = a\n# c\n[other]\nkey=b\nregion = x\n";
        assert_eq!(ini_section(text, "other").unwrap()["key"], "b");
        assert_eq!(ini_section(text, "default").unwrap().len(), 1);
        assert!(ini_section(text, "missing").is_none());
    }

    #[test]
    fn pem_block_extracts_label_and_body() {
        let (label, body) = pem_block(
            "x\n-----BEGIN RSA PRIVATE KEY-----\nAB\nCD\n-----END RSA PRIVATE KEY-----\n",
        )
        .unwrap();
        assert_eq!((label.as_str(), body.as_str()), ("RSA PRIVATE KEY", "ABCD"));
        assert!(
            RsaSigner::from_pem(
                "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAA==\n-----END ENCRYPTED PRIVATE KEY-----"
            )
            .is_err()
        );
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    /// A fresh RSA-2048 PKCS#8 PEM key per test run (no key is committed).
    pub fn test_pem() -> String {
        use aws_lc_rs::encoding::AsDer;
        let pair = aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap();
        let der: aws_lc_rs::encoding::Pkcs8V1Der = pair.as_der().unwrap();
        let body = super::b64(der.as_ref());
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            lines.join("\n")
        )
    }
}
