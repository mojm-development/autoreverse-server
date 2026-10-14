//! Shared application state handed to every handler.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::auth::login::MAX_CONCURRENT_LOGIN_HASHES;
use crate::auth::throttle::LoginThrottle;
use crate::config::Config;

/// Cheap to clone; everything behind it is shared. One per server process — what legacy kept in
/// module-level singletons (throttle, semaphore) lives here, so each test gets its own.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub login_throttle: Arc<LoginThrottle>,
    pub login_semaphore: Arc<Semaphore>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        AppState {
            db,
            config: Arc::new(config),
            login_throttle: Arc::new(LoginThrottle::new()),
            login_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_LOGIN_HASHES)),
        }
    }
}
