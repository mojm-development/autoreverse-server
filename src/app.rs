//! The router factory. `app(state)` is everything the server serves, so tests can drive it with
//! `tower::ServiceExt::oneshot` without binding a port.

use std::any::Any;

use axum::Router;
use axum::extract::{Request, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::error::{INTERNAL_DETAIL, detail_response, error_page};
use crate::state::AppState;
use crate::{api, web};

pub fn app(state: AppState) -> Router {
    let static_files =
        ServeDir::new(&state.config.static_dir).not_found_service(not_found.into_service());
    Router::new()
        .merge(api::router())
        .merge(web::router())
        .fallback_service(static_files)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            web::errors::shell_errors,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            reject_cross_site_forms,
        ))
        .layer(middleware::from_fn(trim_trailing_slash))
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Unknown paths: the error page for browsers, `{detail}` for API clients.
async fn not_found(headers: HeaderMap) -> Response {
    if wants_html(&headers) {
        error_page(StatusCode::NOT_FOUND, "Seite nicht gefunden")
    } else {
        detail_response(StatusCode::NOT_FOUND, "Nicht gefunden")
    }
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"))
}

/// A panicking handler becomes the same opaque 500 as any other uncaught error (legacy
/// `handleError`), instead of a dropped connection.
fn panic_response(_panic: Box<dyn Any + Send + 'static>) -> Response {
    tracing::error!("handler panicked");
    detail_response(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_DETAIL)
}

/// SvelteKit's default `trailingSlash: 'never'`: `/items/` answers 308 to `/items` (query kept),
/// so a client that appends a slash still lands on the route instead of a 404.
async fn trim_trailing_slash(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if path.len() > 1 && path.ends_with('/') {
        // One leading slash only: `//evil.example/` must not become a protocol-relative
        // redirect off-site.
        let trimmed = format!("/{}", path.trim_matches('/'));
        let target = match request.uri().query() {
            Some(query) => format!("{trimmed}?{query}"),
            None => trimmed,
        };
        return (StatusCode::PERMANENT_REDIRECT, [(header::LOCATION, target)]).into_response();
    }
    next.run(request).await
}

/// SvelteKit's CSRF check, which legacy got for free: a form-encoded POST/PUT/PATCH/DELETE whose
/// `Origin` is not this server's own is refused. The server's origin is `ORIGIN` if set, else
/// `http://<Host>`. JSON requests are not affected — a browser cannot send them cross-site without
/// a CORS preflight, which this server never grants.
async fn reject_cross_site_forms(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let unsafe_method = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if unsafe_method && is_form(request.headers()) {
        let headers = request.headers();
        if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            let expected = state.config.origin.clone().or_else(|| {
                let host = headers.get(header::HOST).and_then(|v| v.to_str().ok())?;
                Some(format!("http://{host}"))
            });
            if expected.as_deref() != Some(origin) {
                return (
                    StatusCode::FORBIDDEN,
                    "Cross-site POST form submissions are forbidden",
                )
                    .into_response();
            }
        }
    }
    next.run(request).await
}

fn is_form(headers: &HeaderMap) -> bool {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        content_type.as_str(),
        "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
    )
}
