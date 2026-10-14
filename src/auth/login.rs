//! The one login path behind `POST /auth/login` and the web form (legacy `login.ts`).

use axum::http::StatusCode;
use sqlx::PgPool;
use tokio::sync::Semaphore;

use super::passwords::authenticate;
use super::throttle::LoginThrottle;
use super::tokens::issue_token;
use crate::error::ApiError;

/// At most this many password hashes run for logins at once; more would let a burst of login
/// attempts eat 64 MiB each.
pub const MAX_CONCURRENT_LOGIN_HASHES: usize = 4;

pub async fn perform_login(
    db: &PgPool,
    throttle: &LoginThrottle,
    semaphore: &Semaphore,
    name: &str,
    password: &str,
) -> Result<String, ApiError> {
    let user_id = {
        let Ok(_permit) = semaphore.try_acquire() else {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Gerade zu viele gleichzeitige Anmeldeversuche, bitte kurz erneut versuchen",
            )
            .with_retry_after(1));
        };
        let key = name.to_lowercase();
        if let Some(remaining) = throttle.check_and_record_attempt(&key) {
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
    throttle.record_success(&name.to_lowercase());
    Ok(issue_token(db, user_id).await?)
}
