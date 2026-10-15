//! `GET /me`, `POST /me/password`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post};
use serde::Serialize;
use serde_json::{Map, Value};

use super::validate::ApiJson;
use crate::auth::ApiUser;
use crate::auth::directory::{DirectoryError, reset_password, valid_password};
use crate::auth::passwords::authenticate;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .route("/me/password", post(change_password))
}

#[derive(Serialize, sqlx::FromRow)]
struct MeOut {
    id: i32,
    name: String,
    is_admin: bool,
}

async fn me(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<MeOut>> {
    let row: Option<MeOut> = sqlx::query_as("SELECT id, name, is_admin FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Unbekannter Nutzer"))
}

pub const NEW_PASSWORD_INVALID: &str = "new_password muss 8–200 Zeichen haben";
pub const WRONG_CURRENT_PASSWORD: &str = "Aktuelles Passwort ist falsch";

/// `{current_password, new_password}` → 204. Every other session of the user ends; the one the
/// request came with stays (legacy `me/password`).
async fn change_password(
    State(state): State<AppState>,
    user: ApiUser,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<StatusCode> {
    let new_password = body
        .get("new_password")
        .and_then(Value::as_str)
        .filter(|p| valid_password(p))
        .ok_or_else(|| ApiError::unprocessable(NEW_PASSWORD_INVALID))?;
    let current = body
        .get("current_password")
        .and_then(Value::as_str)
        .unwrap_or("");
    change_own_password(&state, &user, current, new_password).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The check and change behind `POST /me/password` and Settings → Sicherheit. 404 if the user
/// is gone, 401 for a wrong current password.
pub async fn change_own_password(
    state: &AppState,
    user: &ApiUser,
    current: &str,
    new_password: &str,
) -> ApiResult<()> {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;
    let Some(name) = name else {
        return Err(ApiError::not_found("Unbekannter Nutzer"));
    };
    if authenticate(&state.db, &name, current).await? != Some(user.id) {
        return Err(ApiError::unauthorized(WRONG_CURRENT_PASSWORD));
    }
    reset_password(&state.db, user.id, new_password, Some(&user.token))
        .await
        .map_err(|e| match e {
            DirectoryError::Db(e) => ApiError::from(e),
            _ => ApiError::not_found("Unbekannter Nutzer"),
        })
}
