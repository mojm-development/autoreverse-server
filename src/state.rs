//! Shared application state handed to every handler.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::auth::login::MAX_CONCURRENT_LOGIN_HASHES;
use crate::auth::throttle::LoginThrottle;
use crate::config::Config;
use crate::scanner::state::ScanStatus;

const AI_REQUESTS_PER_WINDOW: usize = 10;
const MAX_CONCURRENT_AI_REQUESTS: usize = 2;

/// Cheap to clone; everything behind it is shared. One per server process — what legacy kept in
/// module-level singletons (throttle, semaphore) lives here, so each test gets its own.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub login_throttle: Arc<LoginThrottle>,
    /// The same per client address (see `auth::client_address`).
    pub login_throttle_by_address: Arc<LoginThrottle>,
    pub login_semaphore: Arc<Semaphore>,
    /// Language-model requests per user; each one can cost money.
    pub ai_throttle: Arc<LoginThrottle>,
    pub ai_semaphore: Arc<Semaphore>,
    pub ai_feed_attempts: Arc<std::sync::Mutex<crate::ai::feed::Attempts>>,
    /// Whether a MusicBrainz run is active (one at a time).
    pub musicbrainz_running: Arc<std::sync::atomic::AtomicBool>,
    /// The one scan status of this process (legacy `scanState`).
    pub scan: ScanStatus,
    /// The background pass that measures loudness, and how far it is.
    pub loudness: crate::scanner::loudness::LoudnessJob,
    /// Outgoing HTTP (feeds, episodes, the podcast directory).
    pub http: crate::podcasts::HttpClients,
    /// The radio streams that run right now: slots per user, titles per station.
    pub radio: Arc<crate::stations::Hub>,
    /// Logos of the directory's stations: which id has which, and those fetched lately.
    pub radio_logos: Arc<crate::stations::SearchLogos>,
    /// Remote control: the devices that are online, per account, and their event streams.
    pub devices: Arc<crate::devices::Hub>,
    /// Artists whose catalogue answers are being renewed right now.
    pub artist_info_renewing: crate::library::artist_info::Renewing,
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
            ai_throttle: Arc::new(LoginThrottle::new().with_limit(AI_REQUESTS_PER_WINDOW)),
            ai_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_AI_REQUESTS)),
            ai_feed_attempts: Arc::default(),
            musicbrainz_running: Arc::default(),
            scan: ScanStatus::default(),
            loudness: Default::default(),
            http: crate::podcasts::http_client(),
            radio: Arc::default(),
            radio_logos: Arc::default(),
            devices: Arc::default(),
            artist_info_renewing: Default::default(),
        }
    }
}
