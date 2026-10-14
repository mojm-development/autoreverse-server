//! `POST /auth/login`, `POST /auth/logout`.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};

use super::validate::ApiJson;
use crate::auth::ApiUser;
use crate::auth::login::perform_login;
use crate::auth::session::cleared_session_cookie;
use crate::auth::tokens::revoke_token;
use crate::error::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
}

#[derive(Deserialize)]
struct LoginBody {
    name: String,
    password: String,
}

#[derive(Serialize)]
struct TokenOut {
    token: String,
}

async fn login(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<LoginBody>,
) -> ApiResult<Json<TokenOut>> {
    let token = perform_login(
        &state.db,
        &state.login_throttle,
        &state.login_semaphore,
        &body.name,
        &body.password,
    )
    .await?;
    Ok(Json(TokenOut { token }))
}

/// Revokes the token the request came with — the bearer value if one was sent, else the cookie's —
/// and clears the cookie in case a browser called it.
async fn logout(
    State(state): State<AppState>,
    user: ApiUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    // Legacy took the second word of any Authorization header, falling back to the session token.
    let from_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(' ').nth(1))
        .map(str::to_owned);
    let token = from_header.unwrap_or(user.token);
    revoke_token(&state.db, &token).await?;
    let cookie = cleared_session_cookie(state.config.secure_cookies());
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie.to_string())],
    )
        .into_response())
}
