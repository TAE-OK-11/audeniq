//! AWS KMS and Secrets Manager over the JSON 1.1 protocol, signed with
//! Signature Version 4.
//!
//! Credentials, first match wins: `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`
//! (/ `AWS_SESSION_TOKEN`); the shared credentials file
//! (`AWS_SHARED_CREDENTIALS_FILE` or `~/.aws/credentials`, profile
//! `AWS_PROFILE` or `default`, mode 0600); the EC2 instance role through
//! IMDSv2.
use super::{CloudError, Result, fail, json, send};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::Sha256;
use zeroize::Zeroizing;

pub struct Credentials {
    pub access_key: String,
    pub secret_key: Zeroizing<String>,
    pub session_token: Option<Zeroizing<String>>,
}

fn region_ok(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    matches!(parts.as_slice(), [a, b, n] | [a, _, b, n] if a.len() == 2
        && a.chars().all(|c| c.is_ascii_lowercase())
        && !b.is_empty() && b.chars().all(|c| c.is_ascii_lowercase())
        && n.len() == 1 && n.chars().all(|c| c.is_ascii_digit()))
        && (parts.len() == 3 || parts[1] == "gov")
}

/// The region of a pinned KMS key ARN (`arn:aws:kms:R:ACCOUNT:key/UUID`, no alias).
pub fn kms_key_region(arn: &str) -> Result<&str> {
    let err = || CloudError("a pinned AWS symmetric KMS key ARN is required (no alias)".into());
    let rest = arn.strip_prefix("arn:aws:kms:").ok_or_else(err)?;
    let (region, rest) = rest.split_once(':').ok_or_else(err)?;
    let (account, key) = rest.split_once(':').ok_or_else(err)?;
    let uuid = key.strip_prefix("key/").ok_or_else(err)?;
    let uuid_ok = uuid.len() == 36
        && uuid.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit() && !c.is_ascii_uppercase(),
        });
    if !region_ok(region)
        || account.len() != 12
        || !account.chars().all(|c| c.is_ascii_digit())
        || !uuid_ok
    {
        return Err(err());
    }
    Ok(region)
}

/// The region of a pinned Secrets Manager secret ARN
/// (`arn:aws:secretsmanager:R:ACCOUNT:secret:NAME-XXXXXX`, the full ARN with
/// its random suffix, not a name).
pub fn secret_region(arn: &str) -> Result<&str> {
    let err = || CloudError("a pinned AWS secret ARN is required (full ARN, not a name)".into());
    let rest = arn
        .strip_prefix("arn:aws:secretsmanager:")
        .ok_or_else(err)?;
    let (region, rest) = rest.split_once(':').ok_or_else(err)?;
    let (account, name) = rest.split_once(':').ok_or_else(err)?;
    let name = name.strip_prefix("secret:").ok_or_else(err)?;
    let (base, suffix) = name.rsplit_once('-').ok_or_else(err)?;
    let base_ok = (1..=512).contains(&base.len())
        && base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_+=.@-".contains(c));
    if !region_ok(region)
        || account.len() != 12
        || !account.chars().all(|c| c.is_ascii_digit())
        || !base_ok
        || suffix.len() != 6
        || !suffix.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(err());
    }
    Ok(region)
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// SigV4 `Authorization` header for a request whose `headers` (lower-case
/// names, including `host` and `x-amz-date`) are all signed.
#[allow(clippy::too_many_arguments)]
pub fn authorization(
    creds: &Credentials,
    method: &str,
    path: &str,
    query: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    region: &str,
    service: &str,
    now: DateTime<Utc>,
) -> String {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = &amz_date[..8];
    let mut headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| {
            (
                k.to_ascii_lowercase(),
                v.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
        .collect();
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "{method}\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{}",
        super::sha256_hex(body)
    );
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        super::sha256_hex(canonical.as_bytes())
    );
    let k_secret = Zeroizing::new(format!("AWS4{}", creds.secret_key.as_str()));
    let k_date = hmac(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = crate::hex::encode(hmac(&k_signing, to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        creds.access_key
    )
}

pub struct Client {
    http: reqwest::Client,
    creds: Credentials,
}

impl Client {
    pub async fn from_environment() -> Result<Self> {
        Ok(Self {
            http: super::https_client()?,
            creds: credentials().await?,
        })
    }

    /// One JSON 1.1 call: `POST https://{service}.{region}.amazonaws.com/`.
    async fn call(
        &self,
        service: &str,
        region: &str,
        target: &str,
        body: &Value,
        what: &str,
    ) -> Result<Value> {
        let host = format!("{service}.{region}.amazonaws.com");
        let url = format!("https://{host}/");
        let host = host.as_str();
        let payload = Zeroizing::new(
            serde_json::to_vec(body).map_err(|_| CloudError("encode request".into()))?,
        );
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let content_type = "application/x-amz-json-1.1";
        let mut signed = vec![
            ("content-type", content_type),
            ("host", host),
            ("x-amz-date", amz_date.as_str()),
            ("x-amz-target", target),
        ];
        let token = self.creds.session_token.as_ref().map(|t| t.as_str());
        if let Some(token) = token {
            signed.push(("x-amz-security-token", token));
        }
        let auth = authorization(
            &self.creds,
            "POST",
            "/",
            "",
            &signed,
            &payload,
            region,
            service,
            now,
        );
        let mut request = self
            .http
            .post(url)
            .header("content-type", content_type)
            .header("x-amz-date", &amz_date)
            .header("x-amz-target", target)
            .header("authorization", auth)
            .body(payload.to_vec());
        if let Some(token) = token {
            request = request.header("x-amz-security-token", token);
        }
        json(&send(request, what).await?, what)
    }

    async fn kms(&self, arn: &str, operation: &str, body: Value) -> Result<Value> {
        let region = kms_key_region(arn)?;
        let response = self
            .call(
                "kms",
                region,
                &format!("TrentService.{operation}"),
                &body,
                &format!("AWS KMS {operation}"),
            )
            .await?;
        if response["KeyId"].as_str() != Some(arn) {
            return fail("AWS KMS returned an unexpected key ARN");
        }
        Ok(response)
    }

    pub async fn kms_encrypt(
        &self,
        arn: &str,
        plaintext: &[u8],
        context: &Value,
    ) -> Result<String> {
        let body = serde_json::json!({
            "KeyId": arn, "Plaintext": super::b64(plaintext),
            "EncryptionAlgorithm": "SYMMETRIC_DEFAULT", "EncryptionContext": context,
        });
        let response = self.kms(arn, "Encrypt", body).await?;
        blob(&response, "CiphertextBlob")
    }

    pub async fn kms_generate_without_plaintext(
        &self,
        arn: &str,
        context: &Value,
    ) -> Result<String> {
        let body =
            serde_json::json!({"KeyId": arn, "KeySpec": "AES_256", "EncryptionContext": context});
        let response = self
            .kms(arn, "GenerateDataKeyWithoutPlaintext", body)
            .await?;
        blob(&response, "CiphertextBlob")
    }

    pub async fn kms_decrypt(
        &self,
        arn: &str,
        ciphertext: &str,
        context: &Value,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let body = serde_json::json!({
            "KeyId": arn, "CiphertextBlob": ciphertext,
            "EncryptionAlgorithm": "SYMMETRIC_DEFAULT", "EncryptionContext": context,
        });
        let response = self.kms(arn, "Decrypt", body).await?;
        let plaintext = Zeroizing::new(blob(&response, "Plaintext")?);
        super::unb64(&plaintext, "AWS KMS Decrypt")
    }

    pub async fn secret_get(&self, arn: &str, stage: &str) -> Result<Zeroizing<Vec<u8>>> {
        let region = secret_region(arn)?;
        let body = serde_json::json!({"SecretId": arn, "VersionStage": stage});
        let response = self
            .call(
                "secretsmanager",
                region,
                "secretsmanager.GetSecretValue",
                &body,
                "AWS Secrets Manager read",
            )
            .await?;
        if response["ARN"].as_str() != Some(arn) {
            return fail("AWS returned an unexpected secret ARN");
        }
        if let Some(text) = response["SecretString"].as_str() {
            return Ok(Zeroizing::new(text.as_bytes().to_vec()));
        }
        match response["SecretBinary"].as_str() {
            Some(binary) => super::unb64(binary, "AWS Secrets Manager read"),
            None => fail("AWS secret has no value"),
        }
    }

    pub async fn secret_put(&self, arn: &str, value: &[u8]) -> Result<()> {
        let region = secret_region(arn)?;
        let body = match std::str::from_utf8(value) {
            Ok(text) => serde_json::json!({"SecretId": arn, "SecretString": text}),
            Err(_) => serde_json::json!({"SecretId": arn, "SecretBinary": super::b64(value)}),
        };
        let response = self
            .call(
                "secretsmanager",
                region,
                "secretsmanager.PutSecretValue",
                &body,
                "AWS Secrets Manager write",
            )
            .await?;
        if response["ARN"].as_str() != Some(arn) {
            return fail("AWS returned an unexpected secret ARN");
        }
        Ok(())
    }
}

fn blob(response: &Value, field: &str) -> Result<String> {
    match response[field].as_str() {
        Some(v) if !v.is_empty() => Ok(v.to_string()),
        _ => fail(format!("AWS response has no {field}")),
    }
}

async fn credentials() -> Result<Credentials> {
    if let (Ok(access_key), Ok(secret)) = (
        std::env::var("AWS_ACCESS_KEY_ID"),
        std::env::var("AWS_SECRET_ACCESS_KEY"),
    ) {
        return Ok(Credentials {
            access_key,
            secret_key: Zeroizing::new(secret),
            session_token: std::env::var("AWS_SESSION_TOKEN").ok().map(Zeroizing::new),
        });
    }
    let file = std::env::var_os("AWS_SHARED_CREDENTIALS_FILE")
        .map(std::path::PathBuf::from)
        .or_else(|| super::home().map(|h| h.join(".aws/credentials")));
    if let Some(file) = file.filter(|f| f.exists()) {
        let text = super::read_private_file(&file, "AWS credentials file")?;
        let profile = std::env::var("AWS_PROFILE").unwrap_or_else(|_| "default".into());
        let section = super::ini_section(&text, &profile)
            .ok_or_else(|| CloudError("AWS credentials profile not found".into()))?;
        if let (Some(access_key), Some(secret)) = (
            section.get("aws_access_key_id"),
            section.get("aws_secret_access_key"),
        ) {
            return Ok(Credentials {
                access_key: access_key.clone(),
                secret_key: Zeroizing::new(secret.clone()),
                session_token: section
                    .get("aws_session_token")
                    .cloned()
                    .map(Zeroizing::new),
            });
        }
    }
    imds_credentials().await
}

/// EC2 instance-role credentials through IMDSv2 (session token first).
async fn imds_credentials() -> Result<Credentials> {
    let http = super::metadata_client()?;
    let base = "http://169.254.169.254/latest";
    let token = send(
        http.put(format!("{base}/api/token"))
            .header("x-aws-ec2-metadata-token-ttl-seconds", "300"),
        "AWS credentials (no environment, credentials file or instance role)",
    )
    .await?;
    let token =
        String::from_utf8(token.to_vec()).map_err(|_| CloudError("invalid IMDS token".into()))?;
    let roles = send(
        http.get(format!("{base}/meta-data/iam/security-credentials/"))
            .header("x-aws-ec2-metadata-token", &token),
        "AWS instance role lookup",
    )
    .await?;
    let role = String::from_utf8_lossy(&roles)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if role.is_empty()
        || !role
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+=,.@_-".contains(c))
    {
        return fail("no AWS instance role attached");
    }
    let body = send(
        http.get(format!("{base}/meta-data/iam/security-credentials/{role}"))
            .header("x-aws-ec2-metadata-token", &token),
        "AWS instance role credentials",
    )
    .await?;
    let v = json(&body, "AWS instance role credentials")?;
    match (
        v["AccessKeyId"].as_str(),
        v["SecretAccessKey"].as_str(),
        v["Token"].as_str(),
    ) {
        (Some(a), Some(s), Some(t)) => Ok(Credentials {
            access_key: a.into(),
            secret_key: Zeroizing::new(s.into()),
            session_token: Some(Zeroizing::new(t.into())),
        }),
        _ => fail("AWS instance role credentials are incomplete"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The worked example of the AWS SigV4 documentation (IAM ListUsers).
    #[test]
    fn sigv4_matches_the_documented_example() {
        let creds = Credentials {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: Zeroizing::new("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into()),
            session_token: None,
        };
        let auth = authorization(
            &creds,
            "GET",
            "/",
            "Action=ListUsers&Version=2010-05-08",
            &[
                (
                    "content-type",
                    "application/x-www-form-urlencoded; charset=utf-8",
                ),
                ("host", "iam.amazonaws.com"),
                ("x-amz-date", "20150830T123600Z"),
            ],
            b"",
            "us-east-1",
            "iam",
            Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap(),
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    #[test]
    fn pinned_arns_only() {
        assert_eq!(
            kms_key_region(
                "arn:aws:kms:ap-northeast-2:123456789012:key/12345678-1234-1234-1234-123456789abc"
            )
            .unwrap(),
            "ap-northeast-2"
        );
        assert_eq!(
            kms_key_region(
                "arn:aws:kms:us-gov-west-1:123456789012:key/12345678-1234-1234-1234-123456789abc"
            )
            .unwrap(),
            "us-gov-west-1"
        );
        for bad in [
            "arn:aws:kms:ap-northeast-2:123456789012:alias/payout",
            "arn:aws:kms:ap-northeast-2:12345678901:key/12345678-1234-1234-1234-123456789abc",
            "arn:aws:kms:evil.test:123456789012:key/12345678-1234-1234-1234-123456789abc",
            "arn:aws:kms:ap-northeast-2:123456789012:key/12345678-1234-1234-1234-123456789ABC",
        ] {
            assert!(kms_key_region(bad).is_err(), "{bad}");
        }
        assert_eq!(
            secret_region(
                "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-AbCdEf"
            )
            .unwrap(),
            "ap-northeast-2"
        );
        for bad in [
            "audeniq/db",
            "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db",
            "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-AbC",
            "arn:aws:kms:ap-northeast-2:123456789012:secret:audeniq/db-AbCdEf",
        ] {
            assert!(secret_region(bad).is_err(), "{bad}");
        }
    }
}
