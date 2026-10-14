//! OCI Vault: key management (crypto endpoint) and secrets, with OCI API
//! request signing (RSA-SHA256 HTTP signatures).
//!
//! Credentials (`OCI_CLI_AUTH` = `api_key` | `instance_principal` forces one):
//! - API key: the OCI config file (`OCI_CLI_CONFIG_FILE` or `~/.oci/config`,
//!   profile `OCI_CLI_PROFILE` or `DEFAULT`; `user`, `fingerprint`,
//!   `tenancy`, `region`, `key_file` with an unencrypted PEM key, all 0600).
//! - Instance principal (no config file, on an OCI compute instance — like an
//!   AWS instance role): the instance's certificate from the metadata service
//!   is exchanged at the regional Auth Service for a short-lived session token
//!   bound to a fresh RSA key held only in memory; renewed before it expires.
//!   The instance needs a dynamic group and policies for the vault/secrets.
use super::{CloudError, Result, b64, fail, json, send, unb64};
use serde_json::Value;
use zeroize::Zeroizing;

fn region_ok(region: &str) -> bool {
    let p: Vec<&str> = region.split('-').collect();
    p.len() == 3
        && p[0].len() == 2
        && p[0].chars().all(|c| c.is_ascii_lowercase())
        && !p[1].is_empty()
        && p[1].chars().all(|c| c.is_ascii_lowercase())
        && p[2].len() == 1
        && p[2].chars().all(|c| c.is_ascii_digit())
}

/// `ocid1.<type>.oc1.[region][.future use].<unique id>`: the region is empty
/// for global resources (tenancy, user: `ocid1.user.oc1..aaaa…`), and the
/// future-use part is present only for some types (keys carry their vault).
fn ocid_ok(ocid: &str, kind: &str) -> bool {
    let p: Vec<&str> = ocid.split('.').collect();
    let part = |s: &str, max: usize| {
        (1..=max).contains(&s.len())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    let region_ok = |s: &str| {
        s.len() <= 32
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let regional = !matches!(kind, "tenancy" | "user" | "compartment");
    (p.len() == 5 || p.len() == 6)
        && p[0] == "ocid1"
        && p[1] == kind
        && p[2] == "oc1"
        && region_ok(p[3])
        && (p[3].is_empty() != regional)
        && (p.len() == 5 || part(p[4], 64))
        && part(p[p.len() - 1], 128)
}

/// `<vault>-crypto.kms.<region>.oraclecloud.com/<key OCID>` → (endpoint, OCID).
/// A key OCID alone does not name its vault's crypto endpoint, so both are
/// pinned together; only Oracle crypto endpoints are accepted.
pub fn key_id(id: &str) -> Result<(String, &str)> {
    let err =
        || CloudError("an OCI key id <vault crypto endpoint host>/<key OCID> is required".into());
    let (host, ocid) = id.split_once('/').ok_or_else(err)?;
    let (vault, rest) = host.split_once("-crypto.kms.").ok_or_else(err)?;
    let region = rest.strip_suffix(".oraclecloud.com").ok_or_else(err)?;
    let vault_ok = (1..=64).contains(&vault.len())
        && vault
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !vault_ok || !region_ok(region) || !ocid_ok(ocid, "key") {
        return Err(err());
    }
    Ok((format!("https://{host}"), ocid))
}

pub fn secret_id(id: &str) -> Result<&str> {
    if ocid_ok(id, "vaultsecret") {
        Ok(id)
    } else {
        fail("a pinned OCI vault secret OCID is required")
    }
}

pub struct Signer {
    key_id: String,
    key: super::RsaSigner,
}

impl Signer {
    pub fn new(tenancy: &str, user: &str, fingerprint: &str, key: super::RsaSigner) -> Self {
        Self {
            key_id: format!("{tenancy}/{user}/{fingerprint}"),
            key,
        }
    }

    /// Headers to add (`date`, `authorization`, and for bodies
    /// `x-content-sha256` / `content-type` / `content-length`) for one request.
    pub fn sign(
        &self,
        method: &str,
        host: &str,
        path_and_query: &str,
        body: Option<&[u8]>,
        date: &str,
    ) -> Result<Vec<(String, String)>> {
        let mut headers = vec![("date".to_string(), date.to_string())];
        let mut lines = vec![
            format!("date: {date}"),
            format!(
                "(request-target): {} {path_and_query}",
                method.to_ascii_lowercase()
            ),
            format!("host: {host}"),
        ];
        let mut names = "date (request-target) host".to_string();
        if let Some(body) = body {
            use base64::Engine;
            use sha2::Digest;
            let digest =
                base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(body));
            for (k, v) in [
                ("x-content-sha256", digest),
                ("content-type", "application/json".to_string()),
                ("content-length", body.len().to_string()),
            ] {
                lines.push(format!("{k}: {v}"));
                names.push(' ');
                names.push_str(k);
                headers.push((k.to_string(), v));
            }
        }
        let signature = b64(&self.key.sign(lines.join("\n").as_bytes())?);
        headers.push((
            "authorization".into(),
            format!(
                r#"Signature version="1",keyId="{}",algorithm="rsa-sha256",headers="{names}",signature="{signature}""#,
                self.key_id
            ),
        ));
        Ok(headers)
    }
}

pub struct Client {
    http: reqwest::Client,
    auth: Auth,
    region: String,
}

enum Auth {
    ApiKey(std::sync::Arc<Signer>),
    Instance(InstancePrincipal),
}

/// OCI instance metadata service (v2 requires the `Authorization: Bearer
/// Oracle` header, which a request forwarded from elsewhere would not carry).
const METADATA: &str = "http://169.254.169.254/opc/v2";
/// Renew the session token this long before it expires.
const RENEW_BEFORE_SECS: i64 = 300;

pub struct InstancePrincipal {
    metadata: reqwest::Client,
    metadata_base: String,
    http: reqwest::Client,
    federation: url::Url,
    session: tokio::sync::Mutex<Option<(std::sync::Arc<Signer>, i64)>>,
}

impl InstancePrincipal {
    pub fn new(
        metadata: reqwest::Client,
        metadata_base: &str,
        http: reqwest::Client,
        federation: &str,
    ) -> Result<Self> {
        Ok(Self {
            metadata,
            metadata_base: metadata_base.trim_end_matches('/').to_string(),
            http,
            federation: url::Url::parse(federation)
                .map_err(|_| CloudError("invalid OCI federation endpoint".into()))?,
            session: tokio::sync::Mutex::new(None),
        })
    }

    async fn metadata(&self, path: &str) -> Result<Zeroizing<Vec<u8>>> {
        metadata_get(&self.metadata, &self.metadata_base, path).await
    }

    /// A signer for the current session token, renewed when it nears expiry.
    pub async fn signer(&self) -> Result<std::sync::Arc<Signer>> {
        let mut session = self.session.lock().await;
        let now = chrono::Utc::now().timestamp();
        if let Some((signer, expires)) = session.as_ref()
            && *expires - RENEW_BEFORE_SECS > now
        {
            return Ok(signer.clone());
        }
        let (signer, expires) = self.federate().await?;
        let signer = std::sync::Arc::new(signer);
        *session = Some((signer.clone(), expires));
        Ok(signer)
    }

    /// Exchange the instance certificate for a session token (Auth Service
    /// `POST /v1/x509`, signed with the certificate's key; keyId
    /// `<tenancy>/fed-x509/<SHA-1 fingerprint>`, as Oracle's SDKs do).
    async fn federate(&self) -> Result<(Signer, i64)> {
        let text = |b: Zeroizing<Vec<u8>>| {
            String::from_utf8(b.to_vec())
                .map_err(|_| CloudError("invalid OCI instance certificate".into()))
        };
        let leaf = pem_der(
            &text(self.metadata("identity/cert.pem").await?)?,
            "CERTIFICATE",
        )?;
        let intermediate = pem_der(
            &text(self.metadata("identity/intermediate.pem").await?)?,
            "CERTIFICATE",
        )?;
        let key_pem = Zeroizing::new(text(self.metadata("identity/key.pem").await?)?);
        let leaf_key = super::RsaSigner::from_pem(&key_pem)?;
        let tenancy = tenancy_from_certificate(&leaf)?;
        let fingerprint = sha1_fingerprint(&leaf);
        let session = super::RsaSigner::generate()?;
        let body = serde_json::json!({
            "certificate": b64(&leaf),
            "publicKey": b64(&session.public_key_spki()?),
            "intermediateCertificates": [b64(&intermediate)],
        });
        let request_signer = Signer {
            key_id: format!("{tenancy}/fed-x509/{fingerprint}"),
            key: leaf_key,
        };
        let response = signed(
            &self.http,
            &request_signer,
            reqwest::Method::POST,
            &self.federation,
            Some(&body),
            "OCI instance principal token",
        )
        .await?;
        let token = text_field(&response, "token")?;
        let expires = token_expiry(&token)?;
        Ok((
            Signer {
                key_id: format!("ST${token}"),
                key: session,
            },
            expires,
        ))
    }
}

async fn metadata_get(
    client: &reqwest::Client,
    base: &str,
    path: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    send(
        client
            .get(format!("{}/{path}", base.trim_end_matches('/')))
            .header("authorization", "Bearer Oracle"),
        "OCI instance metadata",
    )
    .await
}

/// The instance's canonical region (`ap-seoul-1`) from the metadata service.
async fn instance_region(client: &reqwest::Client, base: &str) -> Result<String> {
    let instance = json(
        &metadata_get(client, base, "instance/").await?,
        "OCI instance metadata",
    )?;
    let region = instance["canonicalRegionName"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if !region_ok(&region) {
        return fail("OCI instance metadata has no valid region");
    }
    Ok(region)
}

/// DER bytes of a PEM block with this label.
fn pem_der(pem: &str, label: &str) -> Result<Vec<u8>> {
    match super::pem_block(pem) {
        Some((l, body)) if l == label => Ok(unb64(&body, "OCI certificate")?.to_vec()),
        _ => fail(format!("OCI instance metadata returned no {label}")),
    }
}

/// The tenancy OCID an instance certificate names in its subject
/// (`OU=opc-tenant:<OCID>`, or `opc-identity:` on newer certificates). The
/// value is the whole DER string the marker starts: its length comes from
/// the DER header, since the next element's tag (0x31 = '1') would read as
/// part of the OCID.
fn tenancy_from_certificate(der: &[u8]) -> Result<String> {
    for marker in [&b"opc-tenant:"[..], &b"opc-identity:"[..]] {
        let mut from = 0;
        while let Some(found) = der[from..].windows(marker.len()).position(|w| w == marker) {
            let at = from + found;
            from = at + 1;
            // tag, length (short form, or 0x81 + one byte), then the value
            let (tag, len) = match at.checked_sub(3).map(|i| &der[i..at]) {
                Some([tag, 0x81, len]) => (*tag, *len as usize),
                _ if at >= 2 && der[at - 1] < 0x80 => (der[at - 2], der[at - 1] as usize),
                _ => continue,
            };
            // UTF8String, PrintableString, IA5String
            if !matches!(tag, 0x0c | 0x13 | 0x16) || len < marker.len() || at + len > der.len() {
                continue;
            }
            let ocid = String::from_utf8_lossy(&der[at + marker.len()..at + len]).to_string();
            if ocid_ok(&ocid, "tenancy") {
                return Ok(ocid);
            }
        }
    }
    fail("OCI instance certificate names no tenancy")
}

fn sha1_fingerprint(der: &[u8]) -> String {
    use sha1::Digest;
    sha1::Sha1::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// `exp` of the session token (a JWT; its signature is the Auth Service's
/// business — we only need to know when to renew).
fn token_expiry(token: &str) -> Result<i64> {
    use base64::Engine;
    let payload = token
        .split('.')
        .nth(1)
        .and_then(|p| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(p)
                .ok()
        })
        .and_then(|p| serde_json::from_slice::<Value>(&p).ok())
        .ok_or_else(|| CloudError("invalid OCI session token".into()))?;
    payload["exp"]
        .as_i64()
        .ok_or_else(|| CloudError("OCI session token has no expiry".into()))
}

/// One signed OCI request (JSON in, JSON out) to an absolute URL.
async fn signed(
    http: &reqwest::Client,
    signer: &Signer,
    method: reqwest::Method,
    url: &url::Url,
    body: Option<&Value>,
    what: &str,
) -> Result<Value> {
    let payload = match body {
        Some(b) => Some(Zeroizing::new(
            serde_json::to_vec(b).map_err(|_| CloudError("encode request".into()))?,
        )),
        None => None,
    };
    let date = chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string();
    let host = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let path_and_query = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let headers = signer.sign(
        method.as_str(),
        host,
        path_and_query,
        payload.as_deref().map(|p| p.as_slice()),
        &date,
    )?;
    let mut request = http.request(method, url.clone());
    for (k, v) in &headers {
        if k != "content-length" {
            request = request.header(k.as_str(), v.as_str());
        }
    }
    if let Some(p) = payload {
        request = request.body(p.to_vec());
    }
    json(&send(request, what).await?, what)
}

impl Client {
    /// API key from the OCI config file when there is one; otherwise the
    /// instance principal (`OCI_CLI_AUTH` forces either).
    pub async fn from_environment() -> Result<Self> {
        let path = std::env::var_os("OCI_CLI_CONFIG_FILE")
            .map(std::path::PathBuf::from)
            .or_else(|| super::home().map(|h| h.join(".oci/config")));
        match std::env::var("OCI_CLI_AUTH").unwrap_or_default().as_str() {
            "instance_principal" => Self::instance_principal().await,
            "api_key" => {
                Self::api_key(&path.ok_or_else(|| CloudError("OCI config file not found".into()))?)
            }
            "" => match path.filter(|p| p.exists()) {
                Some(p) => Self::api_key(&p),
                None => Self::instance_principal().await.map_err(|e| {
                    CloudError(format!(
                        "OCI credentials: no config file, and no instance principal ({})",
                        e.0
                    ))
                }),
            },
            _ => fail("OCI_CLI_AUTH must be api_key or instance_principal"),
        }
    }

    async fn instance_principal() -> Result<Self> {
        let region = instance_region(&super::metadata_client()?, METADATA).await?;
        let instance = InstancePrincipal::new(
            super::metadata_client()?,
            METADATA,
            super::https_client()?,
            &format!("https://auth.{region}.oraclecloud.com/v1/x509"),
        )?;
        // Fail now (missing dynamic group, blocked metadata) rather than on first use.
        instance.signer().await?;
        Ok(Self {
            http: super::https_client()?,
            auth: Auth::Instance(instance),
            region,
        })
    }

    fn api_key(path: &std::path::Path) -> Result<Self> {
        let text = super::read_private_file(path, "OCI config file")?;
        let profile = std::env::var("OCI_CLI_PROFILE").unwrap_or_else(|_| "DEFAULT".into());
        let c = super::ini_section(&text, &profile)
            .ok_or_else(|| CloudError("OCI config profile not found".into()))?;
        let get = |k: &str| {
            c.get(k)
                .cloned()
                .ok_or_else(|| CloudError(format!("OCI config has no {k}")))
        };
        if c.contains_key("pass_phrase") {
            return fail(
                "encrypted OCI API keys are not supported; use an unencrypted 0600 key file",
            );
        }
        let (user, tenancy, fingerprint, region) = (
            get("user")?,
            get("tenancy")?,
            get("fingerprint")?,
            get("region")?,
        );
        if !ocid_ok(&user, "user") || !ocid_ok(&tenancy, "tenancy") || !region_ok(&region) {
            return fail("invalid OCI config (user, tenancy or region)");
        }
        if !fingerprint
            .split(':')
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
        {
            return fail("invalid OCI key fingerprint");
        }
        let key_file = get("key_file")?;
        let key_path = match key_file.strip_prefix("~/") {
            Some(rest) => super::home()
                .ok_or_else(|| CloudError("HOME is not set".into()))?
                .join(rest),
            None => std::path::PathBuf::from(key_file),
        };
        let pem = super::read_private_file(&key_path, "OCI API key file")?;
        Ok(Self {
            http: super::https_client()?,
            auth: Auth::ApiKey(std::sync::Arc::new(Signer::new(
                &tenancy,
                &user,
                &fingerprint,
                super::RsaSigner::from_pem(&pem)?,
            ))),
            region,
        })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        host: &str,
        path_and_query: &str,
        body: Option<&Value>,
        what: &str,
    ) -> Result<Value> {
        let signer = match &self.auth {
            Auth::ApiKey(signer) => signer.clone(),
            Auth::Instance(instance) => instance.signer().await?,
        };
        let url = url::Url::parse(&format!("https://{host}{path_and_query}"))
            .map_err(|_| CloudError(format!("{what}: invalid URL")))?;
        signed(&self.http, &signer, method, &url, body, what).await
    }

    async fn crypto(&self, id: &str, op: &str, mut body: Value) -> Result<Value> {
        let (endpoint, ocid) = key_id(id)?;
        body["keyId"] = Value::String(ocid.into());
        let host = endpoint.trim_start_matches("https://").to_string();
        let response = self
            .call(
                reqwest::Method::POST,
                &host,
                &format!("/20180608/{op}"),
                Some(&body),
                &format!("OCI KMS {op}"),
            )
            .await?;
        if response["keyId"].as_str() != Some(ocid) {
            return fail("OCI KMS returned an unexpected key OCID");
        }
        Ok(response)
    }

    pub async fn kms_encrypt(&self, id: &str, plaintext: &[u8], aad: &Value) -> Result<String> {
        let body = serde_json::json!({"plaintext": b64(plaintext), "associatedData": aad, "encryptionAlgorithm": "AES_256_GCM"});
        let response = self.crypto(id, "encrypt", body).await?;
        text_field(&response, "ciphertext")
    }

    pub async fn kms_generate_without_plaintext(&self, id: &str, aad: &Value) -> Result<String> {
        let body = serde_json::json!({"includePlaintextKey": false, "keyShape": {"algorithm": "AES", "length": 32}, "associatedData": aad});
        let response = self.crypto(id, "generateDataEncryptionKey", body).await?;
        if response["plaintext"]
            .as_str()
            .is_some_and(|p| !p.is_empty())
        {
            return fail("OCI returned plaintext key material");
        }
        text_field(&response, "ciphertext")
    }

    pub async fn kms_decrypt(
        &self,
        id: &str,
        ciphertext: &str,
        aad: &Value,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let body = serde_json::json!({"ciphertext": ciphertext, "associatedData": aad, "encryptionAlgorithm": "AES_256_GCM"});
        let response = self.crypto(id, "decrypt", body).await?;
        unb64(
            response["plaintext"].as_str().unwrap_or_default(),
            "OCI KMS decrypt",
        )
    }

    pub async fn secret_get(&self, id: &str, stage: &str) -> Result<Zeroizing<Vec<u8>>> {
        secret_id(id)?;
        let host = format!("secrets.vaults.{}.oci.oraclecloud.com", self.region);
        let response = self
            .call(
                reqwest::Method::GET,
                &host,
                &format!("/20190301/secretbundles/{id}?stage={stage}"),
                None,
                "OCI Vault secret read",
            )
            .await?;
        if response["secretId"].as_str() != Some(id) {
            return fail("OCI returned an unexpected secret");
        }
        let content = &response["secretBundleContent"];
        if content["contentType"].as_str() != Some("BASE64") {
            return fail("unsupported OCI secret content type");
        }
        unb64(
            content["content"].as_str().unwrap_or_default(),
            "OCI Vault secret read",
        )
    }

    pub async fn secret_put(&self, id: &str, value: &[u8]) -> Result<()> {
        secret_id(id)?;
        let host = format!("vaults.{}.oci.oraclecloud.com", self.region);
        let body = serde_json::json!({"secretContent": {"contentType": "BASE64", "content": b64(value), "stage": "CURRENT"}});
        let response = self
            .call(
                reqwest::Method::PUT,
                &host,
                &format!("/20180608/secrets/{id}"),
                Some(&body),
                "OCI Vault secret write",
            )
            .await?;
        if response["id"].as_str() != Some(id) {
            return fail("OCI returned an unexpected secret");
        }
        Ok(())
    }
}

fn text_field(response: &Value, field: &str) -> Result<String> {
    match response[field].as_str() {
        Some(v) if !v.is_empty() => Ok(v.into()),
        _ => fail(format!("OCI response has no {field}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const OCID: &str = "ocid1.key.oc1.ap-seoul-1.bbqexamplevault.abuwcljrexamplekey0123456789";

    #[test]
    fn key_id_pins_oracle_crypto_endpoint_and_key_ocid() {
        let id = format!("bbqexamplevault-crypto.kms.ap-seoul-1.oraclecloud.com/{OCID}");
        assert_eq!(
            key_id(&id).unwrap(),
            (
                "https://bbqexamplevault-crypto.kms.ap-seoul-1.oraclecloud.com".into(),
                OCID
            )
        );
        for bad in [
            OCID.to_string(),
            format!("https://{id}"),
            id.replace("oraclecloud.com", "evil.test"),
            id.replace("ocid1.key", "ocid1.vault"),
            format!("{id}/x"),
            id.replace("-crypto.kms", "-management.kms"),
            format!("evil.test#bbqexamplevault-crypto.kms.ap-seoul-1.oraclecloud.com/{OCID}"),
        ] {
            assert!(key_id(&bad).is_err(), "{bad}");
        }
        assert!(secret_id("ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample.secret0123").is_ok());
        // The usual vault secret OCID has no future-use part.
        assert!(secret_id("ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample").is_ok());
        assert!(secret_id("ocid1.vaultsecret.oc1.iad.amaaaaaaexample").is_ok());
        assert!(
            secret_id("ocid1.vaultsecret.oc1..amaaaaaaexample").is_err(),
            "regional"
        );
        assert!(secret_id("ocid1.vaultsecret.oc1.ap-seoul-1").is_err());
        assert!(secret_id(OCID).is_err());
    }

    /// Real OCIDs: global resources (tenancy, user) have an empty region.
    #[test]
    fn ocids_follow_oracles_format() {
        assert!(ocid_ok(
            "ocid1.tenancy.oc1..aaaaaaaaba3pv6wkcr4jqae5f15p2b2m2yt2j6rx32uzr4h25vqstifsfdsq",
            "tenancy"
        ));
        assert!(ocid_ok(
            "ocid1.user.oc1..aaaaaaaa2s5ujhoo3omesofyjlj4fyw4sw7hh2vdcmq3xgwiixmr6nyogxpa",
            "user"
        ));
        assert!(
            !ocid_ok("ocid1.user.oc1.ap-seoul-1.aaaa", "user"),
            "users are global"
        );
        assert!(!ocid_ok("ocid1.user.oc1..AAAA", "user"));
        assert!(!ocid_ok("ocid1.user.oc2..aaaa", "user"));
        assert!(!ocid_ok("ocid1.user.oc1...aaaa", "user"));
        assert!(!ocid_ok("ocid1.tenancy.oc1..aaaa", "user"));
        assert!(ocid_ok(OCID, "key"));
    }

    /// A DER string element (`tag len value`), as in a certificate subject.
    fn der_string(tag: u8, value: &str) -> Vec<u8> {
        let mut v = vec![tag];
        if value.len() >= 0x80 {
            v.push(0x81);
        }
        v.push(value.len() as u8);
        v.extend_from_slice(value.as_bytes());
        v
    }

    /// A subject fragment: the OU string followed by the next RDN's SET tag
    /// (0x31, the character '1'), as in a real certificate.
    pub(super) fn subject(tag: u8, value: &str) -> Vec<u8> {
        let mut der = b"\x30\x82\x01\x00".to_vec();
        der.extend(der_string(tag, value));
        der.extend_from_slice(b"\x31\x0b\x30\x09\x06\x03\x55\x04\x03");
        der
    }

    #[test]
    fn instance_certificate_names_tenancy_and_fingerprint() {
        let der = subject(0x0c, "opc-tenant:ocid1.tenancy.oc1..aaaaaaaatest");
        assert_eq!(
            tenancy_from_certificate(&der).unwrap(),
            "ocid1.tenancy.oc1..aaaaaaaatest"
        );
        let newer = subject(0x13, "opc-identity:ocid1.tenancy.oc1..aaaaaaaanew");
        assert_eq!(
            tenancy_from_certificate(&newer).unwrap(),
            "ocid1.tenancy.oc1..aaaaaaaanew"
        );
        // Long-form length (an OU longer than 127 bytes is not an OCID, so refused).
        let long = subject(
            0x0c,
            &format!("opc-tenant:ocid1.tenancy.oc1..{}", "a".repeat(130)),
        );
        assert!(tenancy_from_certificate(&long).is_err());
        assert!(tenancy_from_certificate(&subject(0x0c, "opc-tenant:ocid1.user.oc1..x")).is_err());
        assert!(
            tenancy_from_certificate(b"opc-tenant:ocid1.tenancy.oc1..aaaa").is_err(),
            "not a DER string"
        );
        assert!(tenancy_from_certificate(b"no tenant").is_err());
        let fp = sha1_fingerprint(b"abc");
        assert_eq!(
            fp,
            "A9:99:3E:36:47:06:81:6A:BA:3E:25:71:78:50:C2:6C:9C:D0:D8:9D"
        );
    }

    #[test]
    fn session_token_expiry_comes_from_the_jwt() {
        use base64::Engine;
        let enc = |v: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        let token = format!(
            "{}.{}.sig",
            enc(r#"{"alg":"RS256"}"#),
            enc(r#"{"exp":1700000000,"sub":"x"}"#)
        );
        assert_eq!(token_expiry(&token).unwrap(), 1_700_000_000);
        assert!(token_expiry("not-a-jwt").is_err());
        assert!(token_expiry(&format!("a.{}.c", enc(r#"{"sub":"x"}"#))).is_err());
    }

    /// The whole instance-principal exchange against a local metadata
    /// service and Auth Service: the token request is signed with the
    /// instance key under `<tenancy>/fed-x509/<fingerprint>`, the session key
    /// sent there signs later requests as `ST$<token>`, and the token is
    /// reused until it nears expiry.
    #[tokio::test]
    async fn instance_principal_federates_and_renews() {
        use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
        use axum::{
            Json, Router,
            extract::State,
            http::{HeaderMap, StatusCode},
            routing::{get, post},
        };
        use base64::Engine;
        use std::sync::{Arc, Mutex};
        let std64 = base64::engine::general_purpose::STANDARD;
        let pem = |label: &str, der: &[u8]| {
            format!(
                "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
                std64.encode(der)
            )
        };
        let leaf_der = subject(0x0c, "opc-tenant:ocid1.tenancy.oc1..aaaaaaaatest");
        let inter_der = b"\x30\x82intermediate".to_vec();
        let key_pem = super::super::tests_support::test_pem();
        let leaf_public = super::super::RsaSigner::from_pem(&key_pem)
            .unwrap()
            .public_key_der();

        #[derive(Clone)]
        struct Fake {
            leaf: String,
            inter: String,
            key: String,
            leaf_der: Vec<u8>,
            leaf_public: Vec<u8>,
            calls: Arc<Mutex<u32>>,
            session_keys: Arc<Mutex<Vec<Vec<u8>>>>,
            lifetime: Arc<Mutex<i64>>,
        }
        let fake = Fake {
            leaf: pem("CERTIFICATE", &leaf_der),
            inter: pem("CERTIFICATE", &inter_der),
            key: key_pem.clone(),
            leaf_der: leaf_der.clone(),
            leaf_public,
            calls: Arc::default(),
            session_keys: Arc::default(),
            lifetime: Arc::new(Mutex::new(1200)),
        };
        let oracle = |h: &HeaderMap| {
            h.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer Oracle")
        };
        async fn x509(
            State(f): State<Fake>,
            h: HeaderMap,
            body: String,
        ) -> (StatusCode, Json<Value>) {
            use base64::Engine;
            let std64 = base64::engine::general_purpose::STANDARD;
            let header = |k: &str| h.get(k).unwrap().to_str().unwrap().to_string();
            let auth = header("authorization");
            let fp = super::sha1_fingerprint(&f.leaf_der);
            assert!(
                auth.contains(&format!(
                    r#"keyId="ocid1.tenancy.oc1..aaaaaaaatest/fed-x509/{fp}""#
                )),
                "{auth}"
            );
            let signature = auth
                .rsplit_once("signature=\"")
                .unwrap()
                .1
                .trim_end_matches('"');
            let signing = format!(
                "date: {}\n(request-target): post /v1/x509\nhost: {}\nx-content-sha256: {}\ncontent-type: application/json\ncontent-length: {}",
                header("date"),
                header("host"),
                header("x-content-sha256"),
                body.len()
            );
            UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &f.leaf_public)
                .verify(signing.as_bytes(), &std64.decode(signature).unwrap())
                .expect("token request signed with the instance key");
            let v: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["certificate"], std64.encode(&f.leaf_der));
            assert_eq!(
                v["intermediateCertificates"][0],
                std64.encode(b"\x30\x82intermediate")
            );
            f.session_keys
                .lock()
                .unwrap()
                .push(std64.decode(v["publicKey"].as_str().unwrap()).unwrap());
            let n = {
                let mut c = f.calls.lock().unwrap();
                *c += 1;
                *c
            };
            let exp = chrono::Utc::now().timestamp() + *f.lifetime.lock().unwrap();
            let enc = |s: String| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
            let token = format!(
                "{}.{}.sig",
                enc(r#"{"alg":"RS256"}"#.into()),
                enc(format!(r#"{{"exp":{exp},"n":{n}}}"#))
            );
            (StatusCode::OK, Json(serde_json::json!({"token": token})))
        }
        let app = Router::new()
            .route(
                "/opc/v2/instance/",
                get(move |h: HeaderMap| async move {
                    assert!(oracle(&h));
                    Json(serde_json::json!({"canonicalRegionName": "ap-seoul-1", "region": "icn"}))
                }),
            )
            .route(
                "/opc/v2/identity/cert.pem",
                get(|State(f): State<Fake>| async move { f.leaf }),
            )
            .route(
                "/opc/v2/identity/intermediate.pem",
                get(|State(f): State<Fake>| async move { f.inter }),
            )
            .route(
                "/opc/v2/identity/key.pem",
                get(move |State(f): State<Fake>, h: HeaderMap| async move {
                    assert!(oracle(&h), "metadata needs the Oracle header");
                    f.key
                }),
            )
            .route("/v1/x509", post(x509))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://{addr}/opc/v2");

        assert_eq!(
            instance_region(&reqwest::Client::new(), &base)
                .await
                .unwrap(),
            "ap-seoul-1"
        );
        let ip = InstancePrincipal::new(
            reqwest::Client::new(),
            &base,
            reqwest::Client::new(),
            &format!("http://{addr}/v1/x509"),
        )
        .unwrap();
        let first = ip.signer().await.unwrap();
        assert!(first.key_id.starts_with("ST$"), "{}", first.key_id);
        // The session key the Auth Service bound to the token signs requests.
        let headers = first.sign("GET", "h.example", "/x", None, "d").unwrap();
        let auth = &headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .unwrap()
            .1;
        let signature = auth
            .rsplit_once("signature=\"")
            .unwrap()
            .1
            .trim_end_matches('"');
        let spki = fake.session_keys.lock().unwrap()[0].clone();
        let public = &spki[spki.len() - 270..]; // RSA-2048 PKCS#1 public key at the end of the SPKI
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public)
            .verify(
                b"date: d\n(request-target): get /x\nhost: h.example",
                &std64.decode(signature).unwrap(),
            )
            .expect("signed with the session key");
        // Reused while valid.
        let again = ip.signer().await.unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(*fake.calls.lock().unwrap(), 1);
        // A token close to expiry is renewed (with a new session key).
        *fake.lifetime.lock().unwrap() = 60;
        *ip.session.lock().await = None;
        ip.signer().await.unwrap();
        ip.signer().await.unwrap();
        assert_eq!(
            *fake.calls.lock().unwrap(),
            3,
            "a near-expiry token is renewed every time"
        );
        let keys = fake.session_keys.lock().unwrap();
        assert_ne!(keys[0], keys[1]);
    }

    /// The signing string follows OCI's HTTP-signature layout and verifies
    /// with the key's public half.
    #[test]
    fn request_signature_covers_target_host_and_body() {
        use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
        use base64::Engine;
        let pem = super::super::tests_support::test_pem();
        let signer = Signer::new(
            "ocid1.tenancy.oc1..t",
            "ocid1.user.oc1..u",
            "aa:bb",
            super::super::RsaSigner::from_pem(&pem).unwrap(),
        );
        let date = "Thu, 05 Jan 2014 21:31:40 GMT";
        let body = br#"{"keyId":"k"}"#;
        let headers = signer
            .sign("POST", "h.example", "/20180608/encrypt", Some(body), date)
            .unwrap();
        let get = |k: &str| {
            headers
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(get("content-length"), body.len().to_string());
        let auth = get("authorization");
        assert!(auth.contains(r#"keyId="ocid1.tenancy.oc1..t/ocid1.user.oc1..u/aa:bb""#));
        assert!(auth.contains(
            r#"headers="date (request-target) host x-content-sha256 content-type content-length""#
        ));
        let signature = auth
            .rsplit_once("signature=\"")
            .unwrap()
            .1
            .trim_end_matches('"');
        let signing = format!(
            "date: {date}\n(request-target): post /20180608/encrypt\nhost: h.example\nx-content-sha256: {}\ncontent-type: application/json\ncontent-length: {}",
            get("x-content-sha256"),
            body.len()
        );
        let key = super::super::RsaSigner::from_pem(&pem).unwrap();
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key.public_key_der())
            .verify(
                signing.as_bytes(),
                &base64::engine::general_purpose::STANDARD
                    .decode(signature)
                    .unwrap(),
            )
            .unwrap();
        let get_headers = signer
            .sign(
                "GET",
                "h.example",
                "/20190301/secretbundles/x?stage=CURRENT",
                None,
                date,
            )
            .unwrap();
        assert!(get_headers.iter().all(|(k, _)| k != "x-content-sha256"));
    }
}
