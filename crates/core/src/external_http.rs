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

/// Global (internet-routable) unicast address: not loopback, private,
/// link-local, CGNAT, documentation, benchmarking, multicast or reserved.
pub fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_unspecified()
                || v.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                || o[0] >= 240)
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let s = v.segments();
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x0064 && s[1] == 0xff9b))
        }
    }
}

/// Resolves names to public addresses only, so a URL someone else names
/// (a client metadata document) cannot reach this host's network. The
/// connection goes to the address checked here (no second lookup).
pub struct PublicOnlyResolver;

impl reqwest::dns::Resolve for PublicOnlyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public_ip(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} has no public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A client for URLs from outside (no proxy, so the address check above is
/// the one that counts; HTTPS only; no redirects).
pub fn public_client_builder() -> reqwest::ClientBuilder {
    client_builder(false)
        .no_proxy()
        .dns_resolver(PublicOnlyResolver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn public_client_never_connects_to_this_network() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = public_client_builder()
            .build()
            .unwrap()
            .get(format!("https://localhost:{port}/client.json"))
            .send()
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("no public address"), "{err:?}");
    }

    #[test]
    fn only_public_addresses_are_public() {
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "192.0.0.1",
            "198.18.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
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
