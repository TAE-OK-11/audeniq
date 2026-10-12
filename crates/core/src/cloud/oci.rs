//! OCI Vault: key management (crypto endpoint) and secrets, with OCI API
//! request signing (RSA-SHA256 HTTP signatures).
//!
//! Credentials: the OCI config file (`OCI_CLI_CONFIG_FILE` or `~/.oci/config`,
//! profile `OCI_CLI_PROFILE` or `DEFAULT`; `user`, `fingerprint`, `tenancy`,
//! `region`, `key_file` with an unencrypted PEM key, all 0600).
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

fn ocid_ok(ocid: &str, kind: &str) -> bool {
    let p: Vec<&str> = ocid.split('.').collect();
    let part = |s: &str, max: usize| {
        (1..=max).contains(&s.len())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    p.len() == 6
        && p[0] == "ocid1"
        && p[1] == kind
        && p[2] == "oc1"
        && (1..=32).contains(&p[3].len())
        && p[3]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && part(p[4], 64)
        && part(p[5], 128)
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
    signer: Signer,
    region: String,
}

impl Client {
    pub fn from_environment() -> Result<Self> {
        let path = std::env::var_os("OCI_CLI_CONFIG_FILE")
            .map(std::path::PathBuf::from)
            .or_else(|| super::home().map(|h| h.join(".oci/config")))
            .ok_or_else(|| CloudError("OCI config file not found".into()))?;
        let text = super::read_private_file(&path, "OCI config file")?;
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
            signer: Signer::new(
                &tenancy,
                &user,
                &fingerprint,
                super::RsaSigner::from_pem(&pem)?,
            ),
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
        let payload = match body {
            Some(b) => Some(Zeroizing::new(
                serde_json::to_vec(b).map_err(|_| CloudError("encode request".into()))?,
            )),
            None => None,
        };
        let date = chrono::Utc::now()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let headers = self.signer.sign(
            method.as_str(),
            host,
            path_and_query,
            payload.as_deref().map(|p| p.as_slice()),
            &date,
        )?;
        let mut request = self
            .http
            .request(method, format!("https://{host}{path_and_query}"));
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
        text(&response, "ciphertext")
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
        text(&response, "ciphertext")
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

fn text(response: &Value, field: &str) -> Result<String> {
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
        assert!(secret_id("ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample").is_err());
        assert!(secret_id(OCID).is_err());
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
