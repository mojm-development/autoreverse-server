//! Outgoing HTTP with a fence around the local network.
//!
//! A feed is entered by a Verwalter, but what it names — episode files, artwork, chapter files —
//! comes from whoever runs the feed. A feed on the internet must not be able to make this server
//! fetch `http://192.168.1.1/…` or the cloud metadata service and then hand the answer out as an
//! "episode". So URLs a public feed names go through a client that refuses every address that is
//! not public; a feed that itself lives in the local network may point there as well.

use std::net::{IpAddr, SocketAddr};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, RequestBuilder, Url, redirect};

/// Whether `ip` is an ordinary internet address: not loopback, private, link-local (cloud
/// metadata lives at 169.254.169.254), carrier-grade NAT, unique-local, multicast or reserved.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (18..20).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// The URL's host as an IP address, if it is written as one.
fn literal_ip(url: &Url) -> Option<IpAddr> {
    url.host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

/// Resolves like the system does, but keeps only public addresses — and fails when none is left,
/// so a host name that points into the local network cannot be reached either.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| is_public(addr.ip()))
                .collect();
            if found.is_empty() {
                return Err(format!("{host} liegt im lokalen Netz").into());
            }
            let addrs: Addrs = Box::new(found.into_iter());
            Ok(addrs)
        })
    }
}

fn builder() -> reqwest::ClientBuilder {
    // ring, the provider sqlx already brings; installing twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .user_agent(concat!("Autoreverse/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(20))
}

/// The two clients for every outgoing request. Dereferences to the unfenced one, which serves
/// what a Verwalter typed in (feeds) and the podcast directory.
#[derive(Clone)]
pub struct HttpClients {
    any: Client,
    public: Client,
}

impl Deref for HttpClients {
    type Target = Client;
    fn deref(&self) -> &Client {
        &self.any
    }
}

/// Redirects are followed as `fetch` did (up to ten). Per-request timeouts sit on the requests;
/// the read timeout here is what bounds an episode download — it may take long, but must not
/// stall.
pub fn http_client() -> HttpClients {
    let fenced_redirects = redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            attempt.error("Zu viele Weiterleitungen")
        } else if literal_ip(attempt.url()).is_some_and(|ip| !is_public(ip)) {
            attempt.error("Weiterleitung ins lokale Netz")
        } else {
            attempt.follow()
        }
    });
    HttpClients {
        any: builder().build().expect("HTTP client"),
        public: builder()
            .dns_resolver(Arc::new(PublicOnly))
            .redirect(fenced_redirects)
            .build()
            .expect("HTTP client"),
    }
}

impl HttpClients {
    /// The client for what the feed at `feed_url` names: fenced unless the feed itself lives in
    /// the local network (or on this machine), where its files will too.
    pub async fn for_feed(&self, feed_url: Option<&str>) -> FeedHttp<'_> {
        let local = match feed_url.and_then(|u| Url::parse(u).ok()) {
            Some(url) => feed_is_local(&url).await,
            None => false,
        };
        FeedHttp {
            client: if local { &self.any } else { &self.public },
            fenced: !local,
        }
    }
}

async fn feed_is_local(url: &Url) -> bool {
    if let Some(ip) = literal_ip(url) {
        return !is_public(ip);
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    // Unresolvable counts as public: the fence then stays up.
    match tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(80))).await {
        Ok(mut addrs) => addrs.any(|addr| !is_public(addr.ip())),
        Err(_) => false,
    }
}

/// A client chosen for one feed's URLs.
pub struct FeedHttp<'a> {
    client: &'a Client,
    fenced: bool,
}

impl FeedHttp<'_> {
    /// A GET for `url`, or `None` when the fence refuses it: an address in the local network
    /// written as such never reaches the resolver, so it is checked here.
    pub fn get(&self, url: &str) -> Option<RequestBuilder> {
        if self.fenced
            && let Ok(parsed) = Url::parse(url)
            && literal_ip(&parsed).is_some_and(|ip| !is_public(ip))
        {
            return None;
        }
        Some(self.client.get(url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(ip: &str) -> bool {
        is_public(ip.parse().unwrap())
    }

    #[test]
    fn only_internet_addresses_are_public() {
        for ip in ["93.184.216.34", "1.1.1.1", "2606:4700::1111"] {
            assert!(public(ip), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!public(ip), "{ip}");
        }
    }

    #[tokio::test]
    async fn a_public_feed_may_not_name_a_local_address() {
        let clients = http_client();
        let fenced = clients
            .for_feed(Some("https://93.184.216.34/feed.xml"))
            .await;
        assert!(fenced.get("http://192.168.1.1/admin").is_none());
        assert!(fenced.get("http://[::1]/x").is_none());
        assert!(fenced.get("https://cdn.example.com/ep.mp3").is_some());
        let local = clients
            .for_feed(Some("http://127.0.0.1:3101/feed.xml"))
            .await;
        assert!(local.get("http://127.0.0.1:3101/ep.mp3").is_some());
    }
}
