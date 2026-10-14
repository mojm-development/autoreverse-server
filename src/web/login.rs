//! `/login` (page + form) and `/logout` (legacy `login/+page.server.ts`, `logout/+page.server.ts`).

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get, routing::post};

use crate::auth::login::perform_login;
use crate::auth::session::{cleared_session_cookie, session_cookie_for};
use crate::auth::tokens::revoke_token;
use crate::auth::{Session, WebUser};
use crate::error::WebResult;
use crate::library::queries::{LibraryCounts, library_counts};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
}

#[derive(Template)]
#[template(path = "pages/login.html")]
struct LoginPage {
    counts: LibraryCounts,
    hostname: String,
    message: Option<String>,
}

async fn render(
    state: &AppState,
    status: StatusCode,
    message: Option<String>,
) -> WebResult<Response> {
    let page = LoginPage {
        counts: library_counts(&state.db).await?,
        hostname: gethostname::gethostname().to_string_lossy().into_owned(),
        message,
    };
    Ok((status, Html(page.render()?)).into_response())
}

async fn login_page(State(state): State<AppState>, session: Session) -> WebResult<Response> {
    if session.user_id.is_some() {
        return Ok(Redirect::to("/library").into_response());
    }
    render(&state, StatusCode::OK, None).await
}

/// Same login path as the API; on failure the page comes back with the API's status and German
/// message, like SvelteKit's `fail(status, { message })`.
async fn login_submit(State(state): State<AppState>, body: Bytes) -> WebResult<Response> {
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let field = |key: &str| form.get(key).map(String::as_str).unwrap_or("");
    match perform_login(
        &state.db,
        &state.login_throttle,
        &state.login_semaphore,
        field("name"),
        field("password"),
    )
    .await
    {
        Ok(token) => {
            let cookie = session_cookie_for(token, state.config.secure_cookies());
            Ok((
                [(header::SET_COOKIE, cookie.to_string())],
                Redirect::to("/library"),
            )
                .into_response())
        }
        Err(e) => render(&state, e.status, Some(e.detail)).await,
    }
}

async fn logout(State(state): State<AppState>, user: WebUser) -> WebResult<Response> {
    revoke_token(&state.db, &user.token).await?;
    let cookie = cleared_session_cookie(state.config.secure_cookies());
    Ok((
        [(header::SET_COOKIE, cookie.to_string())],
        Redirect::to("/login"),
    )
        .into_response())
}
