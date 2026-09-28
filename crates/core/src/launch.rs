//! Pre-launch lock for real DSP transmission.
//!
//! Until the official launch, the real partner adapters exist and are
//! tested, but no request may reach a DSP. One switch opens it:
//! `DSP_LIVE_TRANSMISSION=enabled` on the worker (and API). Anything else,
//! including unset, keeps it locked. Two independent layers enforce it:
//!
//! 1. Routing (`crate::routing`): a CONTRACTED partner is never routable
//!    while locked (`PRE_LAUNCH_LOCKED`), so no delivery job is created.
//! 2. The wire (`crate::transport`, `crate::partners::http`): SFTP, S3 and
//!    HTTP clients refuse every non-loopback destination before any DNS
//!    lookup or connection, so even an operator probe or a stray call cannot
//!    leave the server.
//!
//! The local MockDSP sandbox (MOCK activation, no network) and loopback test
//! servers are unaffected.

pub const ENV: &str = "DSP_LIVE_TRANSMISSION";
pub const LOCKED: &str = "LIVE_TRANSMISSION_LOCKED";

/// True only when real DSP transmission has been opened for launch.
pub fn live_transmission_enabled() -> bool {
    std::env::var(ENV).is_ok_and(|v| v.trim() == "enabled")
}

fn is_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost")
        || h.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// May a partner connection to `host` be opened? Loopback always (test
/// servers, never a DSP); anything else only after launch.
pub fn wire_allowed(host: &str) -> bool {
    is_loopback(host) || live_transmission_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_never_a_dsp() {
        for h in ["127.0.0.1", "localhost", "::1", "[::1]", "127.0.0.2"] {
            assert!(wire_allowed(h), "{h}");
        }
    }

    #[test]
    fn external_hosts_are_locked_before_launch() {
        if live_transmission_enabled() {
            return; // an operator shell with the lock opened
        }
        for h in [
            "sftp.partner.example",
            "10.0.0.5",
            "api.melon.com",
            "8.8.8.8",
        ] {
            assert!(!wire_allowed(h), "{h}");
        }
    }
}
