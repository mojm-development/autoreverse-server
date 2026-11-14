//! `/ai/*`: suggestions for the web UI and the native clients. Generated playlists are only
//! previews; a client saves one through the ordinary `/playlists` routes.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::sync::SemaphorePermit;

use super::validate::ApiJson;
use crate::ai::AiConfig;
use crate::ai::feed::{self, FeedOut};
use crate::ai::music::{self, AiFailure, DiscoveredArtist, GeneratedPlaylist, Suggestions};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const MAX_WISH_CHARS: usize = 500;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/status", get(status))
        .route("/ai/feed", get(feed))
        .route("/ai/feed/refresh", post(refresh_feed))
        .route("/ai/playlist", post(playlist))
        .route("/ai/suggestions", post(suggestions))
        .route("/ai/discover", post(discover))
}

#[derive(Serialize)]
struct StatusOut {
    enabled: bool,
    provider: Option<&'static str>,
    model: Option<String>,
}

async fn status(State(state): State<AppState>, _user: ApiUser) -> Json<StatusOut> {
    let config = state.settings.get().ai();
    Json(StatusOut {
        enabled: config.is_some(),
        provider: config.as_ref().map(|c| c.provider.wire()),
        model: config.map(|c| c.model),
    })
}

/// The configuration and a permit, or the reason not to spend money on this request: requests
/// can cost real money, and the server may be reachable from the internet.
async fn admit<'a>(
    state: &'a AppState,
    user: &ApiUser,
) -> Result<(AiConfig, SemaphorePermit<'a>), ApiError> {
    let config =
        state.settings.get().ai().ok_or_else(|| {
            ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "Keine KI eingerichtet")
        })?;
    if let Some(remaining) = state
        .ai_throttle
        .check_and_record_attempt(&user.id.to_string())
    {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Zu viele KI-Anfragen, bitte später erneut versuchen",
        )
        .with_retry_after(remaining.ceil() as u64));
    }
    let permit = state.ai_semaphore.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die KI ist gerade beschäftigt, bitte kurz erneut versuchen",
        )
        .with_retry_after(5)
    })?;
    Ok((config, permit))
}

/// The cause goes to the log; a client only learns that it did not work.
fn failed(failure: AiFailure) -> ApiError {
    match failure {
        AiFailure::Database(e) => ApiError::internal(e),
        AiFailure::Model(e) => {
            tracing::warn!("[ai] {e}");
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "Die KI hat keine brauchbare Antwort geliefert",
            )
        }
    }
}

async fn playlist(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<GeneratedPlaylist>> {
    let wish = body
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if wish.is_empty() || wish.chars().count() > MAX_WISH_CHARS {
        return Err(ApiError::unprocessable("prompt muss 1–500 Zeichen haben"));
    }
    let (config, _permit) = admit(&state, &user).await?;
    let result = music::playlist(&config, &state.http, &state.db, wish).await;
    Ok(Json(result.map_err(failed)?))
}

async fn suggestions(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<Suggestions>> {
    let (config, _permit) = admit(&state, &user).await?;
    let result = music::suggestions(&config, &state.http, &state.db, user.id).await;
    Ok(Json(result.map_err(failed)?))
}

async fn feed(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<FeedOut>> {
    Ok(Json(feed::load(&state, user.id).await?))
}

/// Generates anew on request; counted like every other request that costs money.
async fn refresh_feed(State(state): State<AppState>, user: ApiUser) -> ApiResult<StatusCode> {
    drop(admit(&state, &user).await?);
    feed::start(&state, user.id, true);
    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
struct DiscoverOut {
    artists: Vec<DiscoveredArtist>,
}

async fn discover(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<DiscoverOut>> {
    let (config, _permit) = admit(&state, &user).await?;
    let artists = music::discover(&config, &state.http, &state.db, user.id)
        .await
        .map_err(failed)?;
    Ok(Json(DiscoverOut { artists }))
}
