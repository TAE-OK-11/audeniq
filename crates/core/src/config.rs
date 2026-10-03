use std::env;

fn optional_feature(value: Option<&str>) -> crate::error::Result<bool> {
    match value {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        _ => Err(crate::error::Error::PolicyGate("FEATURE_FLAG_INVALID")),
    }
}
fn read_feature(name: &str) -> crate::error::Result<bool> {
    match env::var(name) {
        Ok(value) => optional_feature(Some(&value)),
        Err(env::VarError::NotPresent) => optional_feature(None),
        Err(_) => Err(crate::error::Error::PolicyGate("FEATURE_FLAG_INVALID")),
    }
}
pub fn kms_enabled() -> crate::error::Result<bool> {
    read_feature("PAYOUT_KMS_ENABLED")
}
pub fn antivirus_enabled() -> crate::error::Result<bool> {
    read_feature("UPLOAD_AV_ENABLED")
}
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub origin: String,
    pub service_secret: String,
    pub secure_cookie: bool,
    pub bind: String,
    pub session_seconds: i64,
    /// Only in-process integration fixtures may bypass the public DSP submit gate.
    /// Runtime configuration always sets this to false.
    pub test_only_bypass_dsp_gate: bool,
}
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        kms_enabled()?;
        antivirus_enabled()?;
        let mode = env::var("APP_ENV").unwrap_or_else(|_| "development".into());
        anyhow::ensure!(
            matches!(mode.as_str(), "development" | "test" | "production"),
            "invalid APP_ENV"
        );
        let production = mode == "production";
        let origin = env::var("APP_ORIGIN")?;
        let parsed = url::Url::parse(&origin)?;
        anyhow::ensure!(
            parsed.origin().ascii_serialization() == origin,
            "APP_ORIGIN must be an exact origin"
        );
        anyhow::ensure!(
            !production || parsed.scheme() == "https",
            "production requires HTTPS"
        );
        let secret = env::var("EDGE_SERVICE_SECRET")?;
        anyhow::ensure!(
            secret.len() >= 32,
            "EDGE_SERVICE_SECRET must have at least 32 random characters"
        );
        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            origin,
            service_secret: secret,
            secure_cookie: production,
            bind: env::var("API_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into()),
            session_seconds: 43200,
            test_only_bypass_dsp_gate: false,
        })
    }
    pub fn cookie_name(&self) -> &str {
        if self.secure_cookie {
            "__Host-audeniq_session"
        } else {
            "audeniq_session"
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn external_security_services_require_explicit_enablement() {
        assert!(!super::optional_feature(None).unwrap());
        assert!(!super::optional_feature(Some("false")).unwrap());
        assert!(super::optional_feature(Some("true")).unwrap());
        for invalid in ["", "1", "FALSE", "disabled", "true "] {
            assert!(super::optional_feature(Some(invalid)).is_err());
        }
    }
}
