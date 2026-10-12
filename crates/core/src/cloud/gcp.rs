//! GCP Cloud KMS and Secret Manager REST APIs with an OAuth2 access token.
//!
//! Token source, first match wins: `GOOGLE_APPLICATION_CREDENTIALS` or the
//! gcloud ADC file (`~/.config/gcloud/application_default_credentials.json`)
//! holding a service-account key (signed JWT grant) or an authorized user
//! (refresh-token grant); otherwise the GCE metadata server. Only
//! `https://oauth2.googleapis.com/token` is accepted as the token endpoint.
//! `GOOGLE_CLOUD_QUOTA_PROJECT` (or the file's `quota_project_id`) is sent as
//! `X-Goog-User-Project`.
use super::{CloudError, Result, b64, crc32c, fail, json, send, unb64};
use serde_json::Value;
use zeroize::Zeroizing;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

fn name_part(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A full CryptoKey resource name
/// (`projects/P/locations/L/keyRings/R/cryptoKeys/K`).
pub fn crypto_key_name(name: &str) -> Result<&str> {
    let p: Vec<&str> = name.split('/').collect();
    let ok = p.len() == 8
        && p[0] == "projects"
        && name_part(p[1], 63)
        && p[2] == "locations"
        && name_part(p[3], 63)
        && p[3]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && p[4] == "keyRings"
        && name_part(p[5], 63)
        && p[6] == "cryptoKeys"
        && name_part(p[7], 63);
    if ok {
        Ok(name)
    } else {
        fail("a full GCP CryptoKey resource name is required")
    }
}

/// `projects/P/secrets/S` → (project, secret).
pub fn secret_name(name: &str) -> Result<(&str, &str)> {
    let p: Vec<&str> = name.split('/').collect();
    let project_ok = |s: &str| {
        (6..=30).contains(&s.len())
            && s.starts_with(|c: char| c.is_ascii_lowercase())
            && !s.ends_with('-')
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    if p.len() == 4
        && p[0] == "projects"
        && project_ok(p[1])
        && p[2] == "secrets"
        && name_part(p[3], 255)
    {
        Ok((p[1], p[3]))
    } else {
        fail("a GCP secret resource name projects/PROJECT/secrets/SECRET is required")
    }
}

pub struct Client {
    http: reqwest::Client,
    token: Zeroizing<String>,
    quota_project: Option<String>,
}

impl Client {
    pub async fn from_environment() -> Result<Self> {
        let http = super::https_client()?;
        let (token, file_quota) = access_token(&http).await?;
        let quota_project = match std::env::var("GOOGLE_CLOUD_QUOTA_PROJECT")
            .ok()
            .or(file_quota)
        {
            Some(p) if name_part(&p, 63) => Some(p),
            Some(_) => return fail("invalid GCP quota project"),
            None => None,
        };
        Ok(Self {
            http,
            token,
            quota_project,
        })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
        what: &str,
    ) -> Result<Value> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.token.as_str());
        if let Some(project) = &self.quota_project {
            request = request.header("x-goog-user-project", project);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        json(&send(request, what).await?, what)
    }

    /// Encrypt or decrypt with request/response CRC32C checks and the AAD.
    pub async fn kms(
        &self,
        key: &str,
        encrypt: bool,
        data: &[u8],
        aad: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        crypto_key_name(key)?;
        let (op, field) = if encrypt {
            ("encrypt", "plaintext")
        } else {
            ("decrypt", "ciphertext")
        };
        let body = serde_json::json!({
            field: b64(data), format!("{field}Crc32c"): crc32c(data).to_string(),
            "additionalAuthenticatedData": b64(aad),
            "additionalAuthenticatedDataCrc32c": crc32c(aad).to_string(),
        });
        let what = format!("GCP KMS {op}");
        let response = self
            .call(
                reqwest::Method::POST,
                &format!("https://cloudkms.googleapis.com/v1/{key}:{op}"),
                Some(&body),
                &what,
            )
            .await?;
        check_kms_response(key, encrypt, &response)
    }

    pub async fn secret_access(&self, name: &str, version: &str) -> Result<Zeroizing<Vec<u8>>> {
        let (project, secret) = secret_name(name)?;
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{project}/secrets/{secret}/versions/{version}:access"
        );
        let response = self
            .call(reqwest::Method::GET, &url, None, "GCP Secret Manager read")
            .await?;
        check_secret_version(secret, &response)?;
        let data = unb64(
            response["payload"]["data"].as_str().unwrap_or_default(),
            "GCP Secret Manager read",
        )?;
        check_crc(&data, &response["payload"]["dataCrc32c"])?;
        Ok(data)
    }

    pub async fn secret_add_version(&self, name: &str, value: &[u8]) -> Result<()> {
        let (project, secret) = secret_name(name)?;
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{project}/secrets/{secret}:addVersion"
        );
        let body = serde_json::json!({"payload": {"data": b64(value), "dataCrc32c": crc32c(value).to_string()}});
        let response = self
            .call(
                reqwest::Method::POST,
                &url,
                Some(&body),
                "GCP Secret Manager write",
            )
            .await?;
        check_secret_version(secret, &response)
    }
}

pub fn check_kms_response(
    key: &str,
    encrypt: bool,
    response: &Value,
) -> Result<Zeroizing<Vec<u8>>> {
    let field = if encrypt {
        let name = response["name"].as_str().unwrap_or_default();
        let version = name
            .strip_prefix(key)
            .and_then(|v| v.strip_prefix("/cryptoKeyVersions/"));
        if !version.is_some_and(|v| {
            !v.is_empty() && !v.starts_with('0') && v.chars().all(|c| c.is_ascii_digit())
        }) {
            return fail("GCP returned an unexpected key resource");
        }
        if response["verifiedPlaintextCrc32c"] != Value::Bool(true)
            || response["verifiedAdditionalAuthenticatedDataCrc32c"] != Value::Bool(true)
        {
            return fail("GCP did not verify request integrity");
        }
        "ciphertext"
    } else {
        "plaintext"
    };
    let data = unb64(response[field].as_str().unwrap_or_default(), "GCP KMS")?;
    check_crc(&data, &response[format!("{field}Crc32c")])?;
    Ok(data)
}

fn check_secret_version(secret: &str, response: &Value) -> Result<()> {
    // The project appears as its number in responses; the secret must match.
    let name = response["name"].as_str().unwrap_or_default();
    let p: Vec<&str> = name.split('/').collect();
    let ok = p.len() == 6
        && p[0] == "projects"
        && !p[1].is_empty()
        && p[2] == "secrets"
        && p[3] == secret
        && p[4] == "versions"
        && !p[5].is_empty()
        && p[5].chars().all(|c| c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        fail("GCP returned an unexpected secret version")
    }
}

fn check_crc(data: &[u8], value: &Value) -> Result<()> {
    let given = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return fail("missing GCP integrity checksum"),
    };
    if given != crc32c(data).to_string() {
        return fail("GCP integrity checksum mismatch");
    }
    Ok(())
}

async fn access_token(http: &reqwest::Client) -> Result<(Zeroizing<String>, Option<String>)> {
    let file = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            super::home().map(|h| h.join(".config/gcloud/application_default_credentials.json"))
        });
    if let Some(file) = file.filter(|f| f.exists()) {
        let text = super::read_private_file(&file, "GCP credentials file")?;
        let creds: Value = serde_json::from_str(&text)
            .map_err(|_| CloudError("invalid GCP credentials file".into()))?;
        let quota = creds["quota_project_id"].as_str().map(str::to_string);
        let form: Vec<(&str, Zeroizing<String>)> = match creds["type"].as_str() {
            Some("service_account") => {
                if creds["token_uri"].as_str().is_some_and(|u| u != TOKEN_URL) {
                    return fail("unexpected GCP token endpoint in credentials file");
                }
                let email = creds["client_email"]
                    .as_str()
                    .ok_or_else(|| CloudError("GCP service account has no client_email".into()))?;
                let key = creds["private_key"]
                    .as_str()
                    .ok_or_else(|| CloudError("GCP service account has no private_key".into()))?;
                let assertion = service_account_jwt(email, key, chrono::Utc::now().timestamp())?;
                vec![
                    (
                        "grant_type",
                        Zeroizing::new("urn:ietf:params:oauth:grant-type:jwt-bearer".into()),
                    ),
                    ("assertion", assertion),
                ]
            }
            Some("authorized_user") => {
                let field = |k: &str| {
                    creds[k]
                        .as_str()
                        .map(|v| Zeroizing::new(v.to_string()))
                        .ok_or_else(|| CloudError(format!("GCP user credentials have no {k}")))
                };
                vec![
                    ("grant_type", Zeroizing::new("refresh_token".into())),
                    ("client_id", field("client_id")?),
                    ("client_secret", field("client_secret")?),
                    ("refresh_token", field("refresh_token")?),
                ]
            }
            _ => {
                return fail(
                    "unsupported GCP credentials type (service_account or authorized_user)",
                );
            }
        };
        let encoded = Zeroizing::new(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(form.iter().map(|(k, v)| (*k, v.as_str())))
                .finish(),
        );
        let request = http
            .post(TOKEN_URL)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(encoded.as_bytes().to_vec());
        let body = send(request, "GCP token exchange").await?;
        return Ok((token_from(&body)?, quota));
    }
    let metadata = super::metadata_client()?;
    let body = send(
        metadata
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("metadata-flavor", "Google"),
        "GCP credentials (no credentials file or metadata server)",
    )
    .await?;
    Ok((token_from(&body)?, None))
}

fn token_from(body: &[u8]) -> Result<Zeroizing<String>> {
    let v = json(body, "GCP token exchange")?;
    match v["access_token"].as_str() {
        Some(t) if !t.is_empty() && t.len() <= 16384 && t.chars().all(|c| c.is_ascii_graphic()) => {
            Ok(Zeroizing::new(t.into()))
        }
        _ => fail("invalid GCP access token"),
    }
}

/// RS256 JWT for the service-account token grant (one hour).
pub fn service_account_jwt(
    email: &str,
    private_key_pem: &str,
    now: i64,
) -> Result<Zeroizing<String>> {
    use base64::Engine;
    let url = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = url.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = serde_json::json!({"iss": email, "scope": SCOPE, "aud": TOKEN_URL, "iat": now, "exp": now + 3600});
    let signing_input = format!("{header}.{}", url.encode(claims.to_string()));
    let signer = super::RsaSigner::from_pem(private_key_pem)?;
    let signature = signer.sign(signing_input.as_bytes())?;
    Ok(Zeroizing::new(format!(
        "{signing_input}.{}",
        url.encode(signature)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str =
        "projects/audeniq-prod/locations/asia-northeast3/keyRings/backend/cryptoKeys/payout";

    #[test]
    fn resource_names_are_pinned() {
        assert!(crypto_key_name(KEY).is_ok());
        for bad in [
            "http://kms.test",
            &format!("{KEY}/cryptoKeyVersions/1"),
            &format!("{KEY}?q=1"),
            &KEY.replace("payout", "../payout"),
        ] {
            assert!(crypto_key_name(bad).is_err(), "{bad}");
        }
        assert_eq!(
            secret_name("projects/audeniq-prod/secrets/edge").unwrap(),
            ("audeniq-prod", "edge")
        );
        assert!(secret_name("projects/audeniq-prod/secrets/edge/versions/1").is_err());
        assert!(secret_name("projects/AUDENIQ/secrets/edge").is_err());
    }

    #[test]
    fn kms_encrypt_response_is_checked() {
        let ct = b"ciphertext";
        let ok = serde_json::json!({"name": format!("{KEY}/cryptoKeyVersions/7"), "ciphertext": b64(ct),
            "ciphertextCrc32c": crc32c(ct).to_string(), "verifiedPlaintextCrc32c": true,
            "verifiedAdditionalAuthenticatedDataCrc32c": true});
        assert_eq!(check_kms_response(KEY, true, &ok).unwrap().as_slice(), ct);
        let mut variants = vec![];
        for (k, v) in [
            (
                "name",
                Value::String(format!("{KEY}-other/cryptoKeyVersions/1")),
            ),
            ("ciphertextCrc32c", Value::String("0".into())),
            ("verifiedPlaintextCrc32c", Value::Bool(false)),
            (
                "verifiedAdditionalAuthenticatedDataCrc32c",
                Value::Bool(false),
            ),
            ("ciphertextCrc32c", Value::Null),
        ] {
            let mut r = ok.clone();
            r[k] = v;
            variants.push(r);
        }
        for r in variants {
            assert!(check_kms_response(KEY, true, &r).is_err(), "{r}");
        }
    }

    #[test]
    fn secret_responses_are_checked() {
        assert!(
            check_secret_version(
                "edge",
                &serde_json::json!({"name": "projects/123/secrets/edge/versions/4"})
            )
            .is_ok()
        );
        assert!(
            check_secret_version(
                "edge",
                &serde_json::json!({"name": "projects/123/secrets/other/versions/4"})
            )
            .is_err()
        );
        assert!(check_crc(b"x", &serde_json::json!(crc32c(b"x"))).is_ok());
        assert!(check_crc(b"x", &serde_json::json!("1")).is_err());
    }

    #[test]
    fn service_account_jwt_is_rs256_signed_for_the_token_endpoint() {
        use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
        use base64::Engine;
        let pem = super::super::tests_support::test_pem();
        let jwt =
            service_account_jwt("svc@p.iam.gserviceaccount.com", &pem, 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let url = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let claims: Value = serde_json::from_slice(&url.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], TOKEN_URL);
        assert_eq!(claims["exp"], 1_700_003_600);
        let signer = super::super::RsaSigner::from_pem(&pem).unwrap();
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, signer.public_key_der())
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &url.decode(parts[2]).unwrap(),
            )
            .unwrap();
    }
}
