//! Shared application state handed to every handler.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::auth::login::MAX_CONCURRENT_LOGIN_HASHES;
use crate::auth::throttle::LoginThrottle;
use crate::config::Config;
use crate::scanner::state::ScanStatus;

/// Cheap to clone; everything behind it is shared. One per server process — what legacy kept in
/// module-level singletons (throttle, semaphore) lives here, so each test gets its own.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub login_throttle: Arc<LoginThrottle>,
    /// The same per client address (see `auth::login::throttled_address`).
    pub login_throttle_by_address: Arc<LoginThrottle>,
    pub login_semaphore: Arc<Semaphore>,
    /// The one scan status of this process (legacy `scanState`).
    pub scan: ScanStatus,
    /// Outgoing HTTP (feeds, episodes, the podcast directory).
    pub http: crate::podcasts::HttpClients,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        AppState {
            db,
            config: Arc::new(config),
            login_throttle: Arc::new(LoginThrottle::new()),
            login_throttle_by_address: Arc::new(
                LoginThrottle::new().with_limit(crate::auth::login::ATTEMPTS_PER_ADDRESS),
            ),
            login_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_LOGIN_HASHES)),
            scan: ScanStatus::default(),
            http: crate::podcasts::http_client(),
        }
    }
}
