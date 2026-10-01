//! The client address used for rate limiting and audit: the TCP peer, or — only when the
//! peer is a trusted proxy — the first untrusted hop in `X-Forwarded-For`, read right to left.

use std::net::IpAddr;

use ipnet::IpNet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientIp(pub IpAddr);

/// Parses `server.trusted_proxies`: CIDRs, or bare addresses treated as a single host.
///
/// # Errors
///
/// Returns the offending entry when one is neither.
pub fn parse_trusted_proxies(raw: &[String]) -> Result<Vec<IpNet>, String> {
    raw.iter()
        .map(|entry| {
            entry
                .parse::<IpNet>()
                .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| format!("server.trusted_proxies: not a CIDR or address: {entry}"))
        })
        .collect()
}

#[must_use]
pub fn resolve(peer: IpAddr, xff: Option<&str>, trusted: &[IpNet]) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let Some(xff) = xff else {
        return peer;
    };
    for hop in xff.rsplit(',').map(str::trim) {
        match hop.parse::<IpAddr>() {
            Ok(ip) if is_trusted(&ip) => {}
            Ok(ip) => return ip,
            Err(_) => return peer,
        }
    }
    peer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_for() {
        let trusted = parse_trusted_proxies(&["172.30.0.0/24".into()]).unwrap_or_default();
        assert_eq!(
            resolve(ip("203.0.113.9"), Some("1.2.3.4"), &trusted),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn trusted_peer_uses_the_first_untrusted_hop_from_the_right() {
        let trusted =
            parse_trusted_proxies(&["172.30.0.0/24".into(), "10.0.0.5".into()]).unwrap_or_default();
        assert_eq!(
            resolve(
                ip("172.30.0.2"),
                Some("6.6.6.6, 198.51.100.7, 10.0.0.5"),
                &trusted
            ),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn garbage_in_forwarded_for_falls_back_to_the_peer() {
        let trusted = parse_trusted_proxies(&["172.30.0.0/24".into()]).unwrap_or_default();
        assert_eq!(
            resolve(ip("172.30.0.2"), Some("not-an-ip"), &trusted),
            ip("172.30.0.2")
        );
    }

    #[test]
    fn invalid_entry_is_reported() {
        assert!(parse_trusted_proxies(&["nope".into()]).is_err());
    }
}
