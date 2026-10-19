//! The outbound address policy for user-supplied hosts (AI endpoints, push endpoints, SMTP
//! hosts, Mastodon instances, Bluesky PDS hosts): one range table, one resolve-and-filter
//! implementation shared by the public `reqwest` client and SMTP.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use ipnet::{Ipv4Net, Ipv6Net};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::error::HttpError;

/// Which destination addresses an outbound request to a user-supplied host may reach.
#[derive(Clone, Copy, Debug)]
pub struct AddressPolicy {
    /// Lifts the private, loopback, link-local, unique-local, and carrier-grade NAT ranges.
    /// Unspecified, multicast, broadcast, and reserved addresses stay refused.
    pub allow_private: bool,
}

impl AddressPolicy {
    /// Public internet addresses only.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            allow_private: false,
        }
    }
}

/// IPv4 ranges refused even with `allow_private`: "this network" (including unspecified),
/// IETF protocol assignments, benchmarking, multicast, and reserved (including broadcast).
const V4_ALWAYS_FORBIDDEN: [Ipv4Net; 5] = [
    Ipv4Net::new_assert(Ipv4Addr::UNSPECIFIED, 8),
    Ipv4Net::new_assert(Ipv4Addr::new(192, 0, 0, 0), 24),
    Ipv4Net::new_assert(Ipv4Addr::new(198, 18, 0, 0), 15),
    Ipv4Net::new_assert(Ipv4Addr::new(224, 0, 0, 0), 4),
    Ipv4Net::new_assert(Ipv4Addr::new(240, 0, 0, 0), 4),
];

/// IPv4 ranges lifted by `allow_private`: private, carrier-grade NAT, loopback, link local
/// (which holds the cloud metadata address `169.254.169.254`).
const V4_PRIVATE: [Ipv4Net; 6] = [
    Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 0, 0), 8),
    Ipv4Net::new_assert(Ipv4Addr::new(100, 64, 0, 0), 10),
    Ipv4Net::new_assert(Ipv4Addr::new(127, 0, 0, 0), 8),
    Ipv4Net::new_assert(Ipv4Addr::new(169, 254, 0, 0), 16),
    Ipv4Net::new_assert(Ipv4Addr::new(172, 16, 0, 0), 12),
    Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 0, 0), 16),
];

/// IPv6 ranges refused even with `allow_private`: unspecified and multicast.
const V6_ALWAYS_FORBIDDEN: [Ipv6Net; 2] = [
    Ipv6Net::new_assert(Ipv6Addr::UNSPECIFIED, 128),
    Ipv6Net::new_assert(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8),
];

/// IPv6 ranges lifted by `allow_private`: loopback, unique local, link local.
const V6_PRIVATE: [Ipv6Net; 3] = [
    Ipv6Net::new_assert(Ipv6Addr::LOCALHOST, 128),
    Ipv6Net::new_assert(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7),
    Ipv6Net::new_assert(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10),
];

/// NAT64 well-known prefix; its low 32 bits are an IPv4 address, checked as such.
const NAT64: Ipv6Net = Ipv6Net::new_assert(Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96);

/// The address actually checked: IPv4-mapped, IPv4-compatible, and NAT64 IPv6 addresses
/// are checked as their embedded IPv4 address. `::` and `::1` stay IPv6.
fn effective(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = ip else {
        return ip;
    };
    if v6.is_unspecified() || v6.is_loopback() {
        return ip;
    }
    if let Some(v4) = v6.to_ipv4() {
        return IpAddr::V4(v4);
    }
    if NAT64.contains(&v6) {
        let [.., a, b, c, d] = v6.octets();
        return IpAddr::V4(Ipv4Addr::new(a, b, c, d));
    }
    ip
}

/// Whether `ip` may be dialed under `policy`.
#[must_use]
pub fn is_allowed(ip: IpAddr, policy: AddressPolicy) -> bool {
    match effective(ip) {
        IpAddr::V4(v4) => {
            !(V4_ALWAYS_FORBIDDEN.iter().any(|net| net.contains(&v4))
                || (!policy.allow_private && V4_PRIVATE.iter().any(|net| net.contains(&v4))))
        }
        IpAddr::V6(v6) => {
            !(V6_ALWAYS_FORBIDDEN.iter().any(|net| net.contains(&v6))
                || (!policy.allow_private && V6_PRIVATE.iter().any(|net| net.contains(&v6))))
        }
    }
}

/// Name resolution, behind a seam so tests can stub the answers.
#[async_trait::async_trait]
pub(crate) trait Lookup: Send + Sync {
    /// Resolves `host` to socket addresses with `port`.
    async fn lookup(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

/// The system resolver, through `tokio::net::lookup_host`.
pub(crate) struct SystemLookup;

#[async_trait::async_trait]
impl Lookup for SystemLookup {
    async fn lookup(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host((host, port)).await?.collect())
    }
}

/// Why resolve-and-filter failed.
#[derive(Debug)]
pub(crate) enum FilterError {
    /// The lookup itself failed.
    Lookup(io::Error),
    /// The policy refused every address.
    Rejected,
}

/// Resolves `host` (an IP literal is checked directly) and keeps only allowed addresses.
pub(crate) async fn resolve_filtered(
    lookup: &dyn Lookup,
    host: &str,
    port: u16,
    policy: AddressPolicy,
) -> Result<Vec<SocketAddr>, FilterError> {
    let literal = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    let candidates = match literal.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => lookup
            .lookup(host, port)
            .await
            .map_err(FilterError::Lookup)?,
    };
    let kept: Vec<SocketAddr> = candidates
        .into_iter()
        .filter(|candidate| is_allowed(candidate.ip(), policy))
        .collect();
    if kept.is_empty() {
        Err(FilterError::Rejected)
    } else {
        Ok(kept)
    }
}

/// [`resolve_filtered`] mapped to [`HttpError`]: a lookup failure is transient, a refusal is
/// [`HttpError::AddressRejected`].
pub(crate) async fn resolve_checked_with(
    lookup: &dyn Lookup,
    host: &str,
    port: u16,
    policy: AddressPolicy,
) -> Result<Vec<SocketAddr>, HttpError> {
    resolve_filtered(lookup, host, port, policy)
        .await
        .map_err(|err| match err {
            FilterError::Lookup(err) => {
                HttpError::Transient(format!("resolving {host}: {}", err.kind()))
            }
            FilterError::Rejected => HttpError::AddressRejected {
                host: host.to_string(),
            },
        })
}

/// Resolves `host` through the system resolver and keeps only the addresses `policy`
/// allows. The SMTP entry point: connect to one returned address and verify TLS against the
/// original host name.
///
/// # Errors
///
/// Returns [`HttpError::AddressRejected`] when no allowed address remains, or
/// [`HttpError::Transient`] when the lookup fails.
pub async fn resolve_checked(
    host: &str,
    port: u16,
    policy: AddressPolicy,
) -> Result<Vec<SocketAddr>, HttpError> {
    resolve_checked_with(&SystemLookup, host, port, policy).await
}

/// Marker carried inside the resolver's `io::Error`, so [`HttpError::from`] can turn a
/// policy refusal back into [`HttpError::AddressRejected`].
#[derive(Debug, thiserror::Error)]
#[error("address rejected: {host}")]
pub(crate) struct AddressRejectedMarker {
    pub(crate) host: String,
}

/// Finds an [`AddressRejectedMarker`] anywhere in an error's source chain, including inside
/// an `io::Error` (whose `source()` skips its own payload).
pub(crate) fn rejected_host(err: &(dyn std::error::Error + 'static)) -> Option<String> {
    let mut current = Some(err);
    while let Some(err) = current {
        if let Some(marker) = err.downcast_ref::<AddressRejectedMarker>() {
            return Some(marker.host.clone());
        }
        if let Some(marker) = err
            .downcast_ref::<io::Error>()
            .and_then(io::Error::get_ref)
            .and_then(|inner| inner.downcast_ref::<AddressRejectedMarker>())
        {
            return Some(marker.host.clone());
        }
        current = err.source();
    }
    None
}

/// The public client's `reqwest` resolver: resolves through a [`Lookup`] and drops every
/// address the policy forbids, so the check is on the address actually dialed.
pub(crate) struct PolicyResolver {
    lookup: Arc<dyn Lookup>,
    policy: AddressPolicy,
}

impl PolicyResolver {
    pub(crate) fn new(policy: AddressPolicy) -> Self {
        Self {
            lookup: Arc::new(SystemLookup),
            policy,
        }
    }
}

impl Resolve for PolicyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let lookup = Arc::clone(&self.lookup);
        let policy = self.policy;
        let host = name.as_str().to_string();
        Box::pin(async move {
            // Port 0: reqwest substitutes the URL's port or the scheme default.
            match resolve_filtered(lookup.as_ref(), &host, 0, policy).await {
                Ok(addrs) => Ok(Box::new(addrs.into_iter()) as Addrs),
                Err(FilterError::Lookup(err)) => Err(err.into()),
                Err(FilterError::Rejected) => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    AddressRejectedMarker { host },
                )
                .into()),
            }
        })
    }
}
