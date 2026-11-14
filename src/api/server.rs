//! `GET /server/info`, and for admins `GET`/`PATCH /server/settings`.

use axum::extract::State;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::api::validate::ApiJson;
use crate::auth::{ApiAdmin, ApiUser};
use crate::error::ApiResult;
use crate::library::queries::{LibraryCounts, library_counts};
use crate::settings;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/server/info", get(info))
        .route("/server/settings", get(get_settings).patch(patch_settings))
}

/// `GET /server/settings` (new in Rust, admins): what Einstellungen → Server shows. The language
/// model's key is never part of it, only `ai.has_api_key`.
async fn get_settings(State(state): State<AppState>, _admin: ApiAdmin) -> Json<settings::Out> {
    Json(settings::Out::from(&state.settings.get()))
}

/// `PATCH /server/settings` (new in Rust, admins): changes the fields named, in effect at once.
/// `ai.api_key` replaces the key, `null` or `""` removes it, absent keeps it. Answers like `GET`.
async fn patch_settings(
    State(state): State<AppState>,
    _admin: ApiAdmin,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<settings::Out>> {
    let next = settings::change(&state, &body).await?;
    Ok(Json(settings::Out::from(&next)))
}

#[derive(Serialize)]
struct InfoOut {
    #[serde(flatten)]
    counts: LibraryCounts,
    /// Only admins learn how many accounts exist; `null` for everyone else.
    user_count: Option<i64>,
    /// What this server offers beyond the library, for clients to show or hide: internet radio
    /// stations, and whether they can be searched in the public directory.
    radio: bool,
    radio_directory: bool,
    /// Remote control between the user's devices (`/me/devices…`).
    devices: bool,
    /// Where the web UI is reached from outside (`ORIGIN`), for clients that build links to its
    /// pages to share; `null` when the operator did not say. Whoever opens such a link needs an
    /// account here — nothing is public.
    web_url: Option<String>,
}

async fn info(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<InfoOut>> {
    let counts = library_counts(&state.db).await?;
    let (is_admin, user_count): (Option<bool>, i64) = sqlx::query_as(
        "SELECT (SELECT is_admin FROM users WHERE id = $1), (SELECT count(*) FROM users)",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let user_count = is_admin.unwrap_or(false).then_some(user_count);
    let settings = state.settings.get();
    Ok(Json(InfoOut {
        counts,
        user_count,
        radio: settings.radio,
        radio_directory: settings.radio_directory_on(),
        devices: settings.devices,
        web_url: state.config.origin.clone(),
    }))
}
