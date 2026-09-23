use std::env;
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub origin: String,
    pub service_secret: String,
    pub secure_cookie: bool,
    pub bind: String,
    pub session_seconds: i64,
}
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
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
