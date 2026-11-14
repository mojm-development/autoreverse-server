//! Which client a request came from, for the login throttle.
//!
//! `X-Forwarded-For` can be written by anybody, so it only counts when the connection comes from
//! a proxy the operator named in `AUTOREVERSE_TRUSTED_PROXIES`, and then only up to the first
//! address that is not one of those proxies.

use std::net::IpAddr;

/// Addresses and networks (`10.0.0.2`, `172.16.0.0/12`) allowed to report the client's address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<(IpAddr, u8)>);

impl TrustedProxies {
    /// Parses a comma-separated list. Refuses anything it cannot read: a typo must stop the
    /// server rather than silently switch the protection off.
    pub fn parse(list: &str) -> Result<Self, String> {
        let mut networks = Vec::new();
        for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (address, prefix) = match entry.split_once('/') {
                Some((address, prefix)) => (address, Some(prefix)),
                None => (entry, None),
            };
            let address: IpAddr = address.parse().map_err(|_| entry.to_owned())?;
            let address = address.to_canonical();
            let longest = if address.is_ipv4() { 32 } else { 128 };
            let prefix = match prefix {
                Some(prefix) => prefix.parse().map_err(|_| entry.to_owned())?,
                None => longest,
            };
            if prefix > longest {
                return Err(entry.to_owned());
            }
            networks.push((address, prefix));
        }
        Ok(TrustedProxies(networks))
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.0.iter().any(|&(network, prefix)| match (network, ip) {
            (IpAddr::V4(network), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
                u32::from(network) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
                u128::from(network) & mask == u128::from(ip) & mask
            }
            _ => false,
        })
    }
}

/// The address to throttle by, or `None` to leave addresses alone.
///
/// A direct peer only counts with a public address: on a home network many devices can share a
/// private one. Behind a trusted proxy the header is read from the right, because each proxy
/// appends what it saw and everything further left is the client's own claim.
pub fn client_address(
    peer: Option<IpAddr>,
    forwarded_for: Option<&str>,
    trusted: &TrustedProxies,
) -> Option<IpAddr> {
    let peer = peer?.to_canonical();
    if !trusted.contains(peer) {
        return Some(peer).filter(|ip| crate::podcasts::net::is_public(*ip));
    }
    for hop in forwarded_for?.rsplit(',') {
        let hop = hop.trim().parse::<IpAddr>().ok()?.to_canonical();
        if !trusted.contains(hop) {
            return Some(hop);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    fn proxies() -> TrustedProxies {
        TrustedProxies::parse("10.0.0.0/8, 2001:db8::1").unwrap()
    }

    #[test]
    fn refuses_entries_it_cannot_read() {
        for bad in ["nonsense", "10.0.0.1/33", "::1/129", "10.0.0.1/x"] {
            assert!(TrustedProxies::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn matches_networks_and_single_addresses() {
        let list = proxies();
        assert!(list.contains("10.200.0.1".parse().unwrap()));
        assert!(list.contains("::ffff:10.0.0.1".parse().unwrap()));
        assert!(list.contains("2001:db8::1".parse().unwrap()));
        assert!(!list.contains("11.0.0.1".parse().unwrap()));
        assert!(!list.contains("2001:db8::2".parse().unwrap()));
    }

    #[test]
    fn prefix_zero_matches_the_whole_family() {
        let list = TrustedProxies::parse("0.0.0.0/0").unwrap();
        assert!(list.contains("8.8.8.8".parse().unwrap()));
        assert!(!list.contains("2001:db8::2".parse().unwrap()));
    }

    #[test]
    fn ignores_the_header_from_an_untrusted_peer() {
        let found = client_address(ip("8.8.8.8"), Some("9.9.9.9"), &proxies());
        assert_eq!(found, ip("8.8.8.8"));
    }

    #[test]
    fn leaves_a_private_direct_peer_alone() {
        assert_eq!(client_address(ip("192.168.1.5"), None, &proxies()), None);
    }

    #[test]
    fn takes_the_address_the_proxy_appended_not_the_one_the_client_sent() {
        let found = client_address(ip("10.0.0.2"), Some("1.1.1.1, 8.8.8.8"), &proxies());
        assert_eq!(found, ip("8.8.8.8"));
    }

    #[test]
    fn skips_chained_trusted_proxies() {
        let found = client_address(ip("10.0.0.2"), Some("8.8.8.8, 10.0.0.3"), &proxies());
        assert_eq!(found, ip("8.8.8.8"));
    }

    #[test]
    fn counts_a_private_client_seen_through_the_proxy() {
        let found = client_address(ip("10.0.0.2"), Some("192.168.1.5"), &proxies());
        assert_eq!(found, ip("192.168.1.5"));
    }

    #[test]
    fn gives_up_on_a_missing_or_broken_header() {
        assert_eq!(client_address(ip("10.0.0.2"), None, &proxies()), None);
        assert_eq!(
            client_address(ip("10.0.0.2"), Some("junk"), &proxies()),
            None
        );
    }
}
