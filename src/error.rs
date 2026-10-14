//! Error types at the HTTP boundary.
//!
//! API routes answer every failure with `{ "detail": "<German text>" }` (legacy `ApiError` +
//! `apiError()` + `handleError`). Web routes either redirect (a missing session) or render the
//! error page.

use askama::Template;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Serialize;

pub const INTERNAL_DETAIL: &str = "Interner Serverfehler";

/// An API failure with its German `detail`. Internal errors are logged where they are converted
/// and leave the server only as "Interner Serverfehler".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{status}: {detail}")]
pub struct ApiError {
    pub status: StatusCode,
    pub detail: String,
    /// Seconds for the `Retry-After` header (429 and 503).
    pub retry_after: Option<u64>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        ApiError {
            status,
            detail: detail.into(),
            retry_after: None,
        }
    }

    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }
    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, detail)
    }
    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, detail)
    }
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, detail)
    }
    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, detail)
    }
    pub fn unprocessable(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, detail)
    }

    /// Logs `source` and returns the opaque 500 every uncaught error became in legacy.
    pub fn internal(source: impl std::fmt::Display) -> Self {
        tracing::error!("{source}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_DETAIL)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::internal(format_args!("database error: {e}"))
    }
}

#[derive(Serialize)]
struct Detail<'a> {
    detail: &'a str,
}

/// The body every error response carries.
pub fn detail_response(status: StatusCode, detail: &str) -> Response {
    (status, axum::Json(Detail { detail })).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = detail_response(self.status, &self.detail);
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

/// A failure in a server-rendered page.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    /// `throw redirect(303, …)` — e.g. no session.
    #[error("redirect to {0}")]
    Redirect(String),
    /// The error page with a status and a German message (`throw error(404, …)`).
    #[error("{0}: {1}")]
    Page(StatusCode, String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type WebResult<T> = Result<T, WebError>;

impl WebError {
    pub fn redirect(to: impl Into<String>) -> Self {
        WebError::Redirect(to.into())
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        WebError::Page(StatusCode::NOT_FOUND, message.into())
    }
}

impl From<sqlx::Error> for WebError {
    fn from(e: sqlx::Error) -> Self {
        WebError::Internal(format!("database error: {e}"))
    }
}

impl From<askama::Error> for WebError {
    fn from(e: askama::Error) -> Self {
        WebError::Internal(format!("template error: {e}"))
    }
}

impl From<ApiError> for WebError {
    fn from(e: ApiError) -> Self {
        // Already logged by `ApiError::internal` if it was one.
        WebError::Page(e.status, e.detail)
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        match self {
            WebError::Redirect(to) => Redirect::to(&to).into_response(),
            WebError::Page(status, message) => error_page(status, &message),
            WebError::Internal(source) => {
                tracing::error!("{source}");
                error_page(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_DETAIL)
            }
        }
    }
}

#[derive(Template)]
#[template(path = "pages/error.html")]
struct ErrorPage<'a> {
    status: u16,
    message: &'a str,
}

/// The standalone error page (legacy `+error.svelte`).
pub fn error_page(status: StatusCode, message: &str) -> Response {
    let page = ErrorPage {
        status: status.as_u16(),
        message,
    };
    match page.render() {
        Ok(html) => {
            let mut response = (status, Html(html)).into_response();
            // Lets web::errors put the page into the app shell where legacy had a layout.
            response
                .extensions_mut()
                .insert(crate::web::errors::ErrorPageInfo {
                    status,
                    message: message.to_owned(),
                });
            response
        }
        Err(e) => {
            tracing::error!("rendering the error page failed: {e}");
            (status, message.to_owned()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_becomes_a_header() {
        let response = ApiError::new(StatusCode::TOO_MANY_REQUESTS, "x")
            .with_retry_after(12)
            .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "12");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    }
}
