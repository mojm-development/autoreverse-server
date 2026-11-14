//! The one login path behind `POST /auth/login` and the web form (legacy `login.ts`).

use std::net::IpAddr;

use axum::http::StatusCode;
use sqlx::PgPool;
use tokio::sync::Semaphore;

use super::client_address::client_address;
use super::passwords::authenticate;
use super::throttle::LoginThrottle;
use super::tokens::issue_token;
use crate::error::ApiError;

/// At most this many password hashes run for logins at once; more would let a burst of login
/// attempts eat 64 MiB each.
pub const MAX_CONCURRENT_LOGIN_HASHES: usize = 4;

/// Login attempts per client address in the throttle's window. Far above the per-name limit: it
/// is meant for one machine trying many names (password spraying), which the per-name throttle
/// never sees, not for a household behind one router mistyping.
pub const ATTEMPTS_PER_ADDRESS: usize = 30;

/// The client address to throttle by, if one is known (see [`client_address`]). `None` when the
/// router is driven without a listener, as in the tests.
pub struct Peer(pub Option<IpAddr>);

impl axum::extract::FromRequestParts<crate::state::AppState> for Peer {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|info| info.0.ip());
        // One list across repeated headers, as the HTTP spec defines it.
        let forwarded = parts
            .headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect::<Vec<_>>()
            .join(",");
        let forwarded = Some(forwarded.as_str()).filter(|f| !f.is_empty());
        Ok(Peer(client_address(
            peer,
            forwarded,
            &state.config.trusted_proxies,
        )))
    }
}

/// The per-address throttle and the address to count, when the peer has a public one.
pub fn address_guard<'a>(
    state: &'a crate::state::AppState,
    peer: &Peer,
) -> Option<(&'a LoginThrottle, IpAddr)> {
    peer.0.map(|ip| (&*state.login_throttle_by_address, ip))
}

pub async fn perform_login(
    db: &PgPool,
    throttle: &LoginThrottle,
    semaphore: &Semaphore,
    name: &str,
    password: &str,
    address: Option<(&LoginThrottle, IpAddr)>,
) -> Result<String, ApiError> {
    // With a known address the count is per address and name, so that a stranger hammering a
    // name only locks themselves out and not its owner.
    let key = match address {
        Some((_, ip)) => format!("{ip}|{}", name.to_lowercase()),
        None => name.to_lowercase(),
    };
    let user_id = {
        let Ok(_permit) = semaphore.try_acquire() else {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Gerade zu viele gleichzeitige Anmeldeversuche, bitte kurz erneut versuchen",
            )
            .with_retry_after(1));
        };
        // Counted on success too: an attacker holding one valid account must not be able to
        // reset the address's count by logging into it in between.
        let by_address =
            address.and_then(|(throttle, ip)| throttle.check_and_record_attempt(&ip.to_string()));
        if let Some(remaining) = by_address.or_else(|| throttle.check_and_record_attempt(&key)) {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "Zu viele Fehlversuche, bitte später erneut versuchen",
            )
            .with_retry_after(remaining.ceil() as u64));
        }
        authenticate(db, name, password).await?
        // The permit is released here, before the token is issued.
    };
    let Some(user_id) = user_id else {
        return Err(ApiError::unauthorized("Anmeldung fehlgeschlagen"));
    };
    throttle.record_success(&key);
    Ok(issue_token(db, user_id).await?)
}
