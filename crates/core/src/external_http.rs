//! External HTTP traffic requires verified TLS. Loopback HTTP is reserved
//! for explicitly enabled development fixtures, never production.
use std::time::Duration;

pub fn local_http_enabled(requested: bool) -> bool {
    requested && std::env::var("APP_ENV").as_deref() != Ok("production")
}

pub fn validate_url(raw: &str, allow_local_http: bool) -> anyhow::Result<url::Url> {
    let url = url::Url::parse(raw)?;
    let loopback = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    anyhow::ensure!(
        url.scheme() == "https" || (allow_local_http && loopback && url.scheme() == "http"),
        "external HTTP endpoints require HTTPS"
    );
    anyhow::ensure!(
        url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "invalid external HTTP endpoint"
    );
    Ok(url)
}

pub fn client_builder(allow_local_http: bool) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .https_only(!allow_local_http)
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .redirect(reqwest::redirect::Policy::none())
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(90))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_endpoints_require_tls_even_when_local_fixtures_are_enabled() {
        assert!(validate_url("https://partner.example/v1", false).is_ok());
        for raw in [
            "http://partner.example/v1",
            "http://192.0.2.1/",
            "https://user:secret@partner.example/",
            "https://partner.example/#x",
        ] {
            assert!(validate_url(raw, true).is_err(), "{raw}");
        }
        for raw in [
            "http://127.0.0.1:7070/",
            "http://localhost:8080/",
            "http://[::1]:7070/",
        ] {
            assert!(validate_url(raw, true).is_ok());
            assert!(validate_url(raw, false).is_err());
        }
    }
    #[tokio::test]
    async fn client_rejects_plaintext_before_opening_a_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = client_builder(false)
            .build()
            .unwrap()
            .get(format!("http://{}/", listener.local_addr().unwrap()))
            .send()
            .await;
        assert!(result.unwrap_err().is_builder());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    }
}
