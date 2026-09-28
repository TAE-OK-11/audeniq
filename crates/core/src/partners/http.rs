//! Authenticated JSON client for partner REST APIs and status endpoints.
use crate::error::{Error, Result};
use crate::partner_config::{HttpApiConfig, HttpAuth, Secret};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// How a failed HTTP exchange must be treated by the no-duplicate rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpFailure {
    /// The request never reached the partner (connect/DNS/TLS) or the
    /// partner refused it before processing (429/502/503/504). Retry-safe.
    NotReceived(String),
    /// The partner answered with a definite rejection (4xx).
    Rejected { status: u16, code: String },
    /// Sent, answer unknown (timeout mid-request, 500, broken body).
    Unknown(String),
}

enum ResolvedAuth {
    Bearer(Secret),
    ApiKey(String, Secret),
    Oauth2 {
        token_url: String,
        client_id: Secret,
        client_secret: Secret,
        scope: Option<String>,
    },
    Hmac {
        key_id: String,
        secret: Secret,
        header: String,
    },
}

pub struct ApiClient {
    pub config: HttpApiConfig,
    auth: ResolvedAuth,
    client: reqwest::Client,
    token: Mutex<Option<(String, Instant)>>,
}

pub fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("hmac key");
    m.update(msg);
    hex::encode(m.finalize().into_bytes())
}

pub fn fill(template: &str, id: &str, name: &str, key: &str) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    template
        .replace("{id}", &enc(id))
        .replace("{name}", &enc(name))
        .replace("{key}", &enc(key))
}

impl ApiClient {
    pub fn new(config: HttpApiConfig) -> Result<Self> {
        let auth = match &config.auth {
            HttpAuth::Bearer { token } => ResolvedAuth::Bearer(token.resolve()?),
            HttpAuth::ApiKey { header, key } => {
                ResolvedAuth::ApiKey(header.clone(), key.resolve()?)
            }
            HttpAuth::Oauth2 {
                token_url,
                client_id,
                client_secret,
                scope,
            } => ResolvedAuth::Oauth2 {
                token_url: token_url.clone(),
                client_id: client_id.resolve()?,
                client_secret: client_secret.resolve()?,
                scope: scope.clone(),
            },
            HttpAuth::Hmac {
                key_id,
                secret,
                header,
            } => ResolvedAuth::Hmac {
                key_id: key_id.clone(),
                secret: secret.resolve()?,
                header: header.clone(),
            },
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(config.timeout_secs.clamp(5, 3600)))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("audeniq-delivery/1")
            .build()
            .map_err(|_| Error::Internal)?;
        Ok(Self {
            config,
            auth,
            client,
            token: Mutex::new(None),
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.base_url.trim_end_matches('/'), path)
    }

    async fn oauth_token(&self) -> std::result::Result<String, HttpFailure> {
        let ResolvedAuth::Oauth2 {
            token_url,
            client_id,
            client_secret,
            scope,
        } = &self.auth
        else {
            return Err(HttpFailure::NotReceived("not oauth2".into()));
        };
        let token_host = url::Url::parse(token_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        if !crate::launch::wire_allowed(&token_host) {
            return Err(HttpFailure::NotReceived(crate::launch::LOCKED.into()));
        }
        let mut cached = self.token.lock().await;
        if let Some((t, until)) = cached.as_ref()
            && Instant::now() < *until
        {
            return Ok(t.clone());
        }
        let mut form = vec![("grant_type", "client_credentials".to_string())];
        if let Some(s) = scope {
            form.push(("scope", s.clone()));
        }
        let form_body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form.iter().map(|(k, v)| (*k, v.as_str())))
            .finish();
        let r = self
            .client
            .post(token_url)
            .basic_auth(client_id.expose(), Some(client_secret.expose()))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body)
            .send()
            .await
            .map_err(|e| HttpFailure::NotReceived(format!("token endpoint: {e}")))?;
        if !r.status().is_success() {
            return Err(HttpFailure::NotReceived(format!(
                "token endpoint {}",
                r.status()
            )));
        }
        let v: Value = r
            .json()
            .await
            .map_err(|e| HttpFailure::NotReceived(e.to_string()))?;
        let t = v
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| HttpFailure::NotReceived("no access_token".into()))?
            .to_string();
        let ttl = v.get("expires_in").and_then(Value::as_u64).unwrap_or(300);
        *cached = Some((
            t.clone(),
            Instant::now() + Duration::from_secs(ttl.saturating_sub(30).max(10)),
        ));
        Ok(t)
    }

    /// One request. `body` is JSON / raw bytes; `idempotency_key` is sent
    /// as `Idempotency-Key` so partners that honour it deduplicate retries.
    pub async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<(Vec<u8>, &str)>,
        idempotency_key: Option<&str>,
    ) -> std::result::Result<(u16, Vec<u8>), HttpFailure> {
        match body {
            Some((bytes, ct)) => {
                let sha = crate::domain::sha256_hex(&bytes);
                let len = bytes.len() as u64;
                self.exchange(
                    method,
                    path,
                    Some((reqwest::Body::from(bytes), len, sha, ct)),
                    idempotency_key,
                )
                .await
            }
            None => self.exchange(method, path, None, idempotency_key).await,
        }
    }

    /// Stream a local file (masters never sit in memory). `sha256` is the
    /// file's pinned hash, used for the HMAC body digest.
    pub async fn send_file(
        &self,
        method: reqwest::Method,
        path: &str,
        local: &std::path::Path,
        sha256: &str,
        content_type: &str,
        idempotency_key: Option<&str>,
    ) -> std::result::Result<(u16, Vec<u8>), HttpFailure> {
        let f = tokio::fs::File::open(local)
            .await
            .map_err(|e| HttpFailure::NotReceived(e.to_string()))?;
        let len = f
            .metadata()
            .await
            .map_err(|e| HttpFailure::NotReceived(e.to_string()))?
            .len();
        self.exchange(
            method,
            path,
            Some((
                reqwest::Body::from(f),
                len,
                sha256.to_string(),
                content_type,
            )),
            idempotency_key,
        )
        .await
    }

    async fn exchange(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<(reqwest::Body, u64, String, &str)>,
        idempotency_key: Option<&str>,
    ) -> std::result::Result<(u16, Vec<u8>), HttpFailure> {
        let url = self.url(path);
        // Pre-launch lock (crate::launch): no request to a real DSP API.
        let host = url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        if !crate::launch::wire_allowed(&host) {
            return Err(HttpFailure::NotReceived(crate::launch::LOCKED.into()));
        }
        let mut req = self.client.request(method.clone(), &url);
        let body_sha = body
            .as_ref()
            .map(|(_, _, sha, _)| sha.clone())
            .unwrap_or_else(|| crate::domain::sha256_hex(b""));
        if let Some((_, len, _, ct)) = &body {
            req = req
                .header("content-type", *ct)
                .header("content-length", *len);
        }
        if let Some(k) = idempotency_key {
            req = req.header("Idempotency-Key", k);
        }
        req = match &self.auth {
            ResolvedAuth::Bearer(t) => req.bearer_auth(t.expose()),
            ResolvedAuth::ApiKey(h, k) => req.header(h.as_str(), k.expose()),
            ResolvedAuth::Oauth2 { .. } => req.bearer_auth(self.oauth_token().await?),
            ResolvedAuth::Hmac {
                key_id,
                secret,
                header,
            } => {
                let ts = chrono::Utc::now().timestamp().to_string();
                let path_only = url::Url::parse(&url)
                    .map(|u| {
                        let mut p = u.path().to_string();
                        if let Some(q) = u.query() {
                            p.push('?');
                            p.push_str(q);
                        }
                        p
                    })
                    .unwrap_or_default();
                let msg = format!("{ts}.{}.{path_only}.{body_sha}", method.as_str());
                req.header("X-Key-Id", key_id.as_str())
                    .header("X-Timestamp", ts)
                    .header(
                        header.as_str(),
                        hmac_hex(secret.expose().as_bytes(), msg.as_bytes()),
                    )
            }
        };
        if let Some((b, _, _, _)) = body {
            req = req.body(b);
        }
        // Only a failed connect (or a request that could not be built) is
        // provably unsent; a reset or timeout after connecting may follow a
        // request the partner already processed.
        let r = req.send().await.map_err(|e| {
            if e.is_connect() || e.is_builder() {
                HttpFailure::NotReceived(e.to_string())
            } else {
                HttpFailure::Unknown(e.to_string())
            }
        })?;
        let status = r.status().as_u16();
        let body = r
            .bytes()
            .await
            .map_err(|e| HttpFailure::Unknown(e.to_string()))?
            .to_vec();
        match status {
            200..=299 => Ok((status, body)),
            429 | 502 | 503 | 504 => Err(HttpFailure::NotReceived(format!("HTTP {status}"))),
            400..=499 => {
                let code = serde_json::from_slice::<Value>(&body)
                    .ok()
                    .and_then(|v| {
                        ["/code", "/error/code", "/error", "/message"]
                            .iter()
                            .find_map(|p| v.pointer(p).and_then(Value::as_str).map(str::to_owned))
                    })
                    .unwrap_or_else(|| format!("HTTP_{status}"));
                Err(HttpFailure::Rejected {
                    status,
                    code: code.chars().take(80).collect(),
                })
            }
            _ => Err(HttpFailure::Unknown(format!("HTTP {status}"))),
        }
    }

    pub async fn json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        idempotency_key: Option<&str>,
    ) -> std::result::Result<Value, HttpFailure> {
        let b = body.map(|v| {
            (
                serde_json::to_vec(v).unwrap_or_default(),
                "application/json",
            )
        });
        let (_, bytes) = self.send(method, path, b, idempotency_key).await?;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|e| HttpFailure::Unknown(e.to_string()))
    }

    /// Normalized status of a partner submission document.
    pub fn status_of(&self, v: &Value) -> (String, Option<String>) {
        let raw = v
            .pointer(&self.config.status_pointer)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let mapped = self
            .config
            .status_map
            .get(&raw)
            .cloned()
            .unwrap_or_else(|| {
                if raw.is_empty() {
                    "PENDING".into()
                } else {
                    format!("REJECTED:{raw}")
                }
            });
        let release_id = v
            .pointer(&self.config.release_id_pointer)
            .and_then(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            });
        (mapped, release_id)
    }
}

/// Verify an inbound webhook signature: `hex(hmac(secret, ts + "." + body))`
/// (or of the body alone without a timestamp header), constant-time, with
/// a replay window on the timestamp.
pub fn verify_webhook(
    secret: &str,
    body: &[u8],
    signature: &str,
    timestamp: Option<&str>,
    max_skew_secs: i64,
    now: i64,
) -> bool {
    let sig = signature
        .trim()
        .trim_start_matches("sha256=")
        .to_ascii_lowercase();
    let expected = match timestamp {
        Some(ts) => {
            let Ok(t) = ts.trim().parse::<i64>() else {
                return false;
            };
            if (now - t).abs() > max_skew_secs {
                return false;
            }
            let mut msg = format!("{}.", ts.trim()).into_bytes();
            msg.extend_from_slice(body);
            hmac_hex(secret.as_bytes(), &msg)
        }
        None => hmac_hex(secret.as_bytes(), body),
    };
    crate::auth::secret_eq(&sig, &expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_signature_checks_body_timestamp_and_window() {
        let body = br#"{"event_id":"e1","type":"accepted"}"#;
        let ts = "1790000000";
        let mut msg = format!("{ts}.").into_bytes();
        msg.extend_from_slice(body);
        let sig = hmac_hex(b"s3cret", &msg);
        assert!(verify_webhook(
            "s3cret",
            body,
            &sig,
            Some(ts),
            300,
            1790000100
        ));
        assert!(verify_webhook(
            "s3cret",
            body,
            &format!("sha256={sig}"),
            Some(ts),
            300,
            1790000100
        ));
        assert!(!verify_webhook(
            "s3cret",
            body,
            &sig,
            Some(ts),
            300,
            1790001000
        ));
        assert!(!verify_webhook(
            "other",
            body,
            &sig,
            Some(ts),
            300,
            1790000100
        ));
        assert!(!verify_webhook(
            "s3cret",
            b"{}",
            &sig,
            Some(ts),
            300,
            1790000100
        ));
        let plain = hmac_hex(b"s3cret", body);
        assert!(verify_webhook("s3cret", body, &plain, None, 300, 0));
    }

    #[tokio::test]
    async fn real_partner_apis_are_not_called_before_launch() {
        if crate::launch::live_transmission_enabled() {
            return;
        }
        // SAFETY: unique variable read only by this test.
        unsafe { std::env::set_var("AUDENIQ_TEST_LOCK_TOKEN", "t") };
        let cfg: crate::partner_config::HttpApiConfig = serde_json::from_value(serde_json::json!({
            "base_url": "https://api.partner.example",
            "auth": {"kind": "bearer", "token": {"env": "AUDENIQ_TEST_LOCK_TOKEN"}}
        }))
        .unwrap();
        let api = ApiClient::new(cfg).unwrap();
        assert_eq!(
            api.send(
                reqwest::Method::POST,
                "/v1/releases",
                Some((b"{}".to_vec(), "application/json")),
                None
            )
            .await,
            Err(HttpFailure::NotReceived(crate::launch::LOCKED.into()))
        );
    }

    #[test]
    fn templates_encode_ids() {
        assert_eq!(
            fill(
                "/v1/r/{id}/files/{name}?k={key}",
                "a/b",
                "x y.flac",
                "delivery:1:2"
            ),
            "/v1/r/a%2Fb/files/x+y.flac?k=delivery%3A1%3A2"
        );
    }
}
