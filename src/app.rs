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
    web::assets::init(&state.config.static_dir);
    // Only the static files answer under `/v/<version>/`, pages do not.
    let static_files = Router::new()
        .fallback_service(
            ServeDir::new(&state.config.static_dir).not_found_service(not_found.into_service()),
        )
        .layer(middleware::from_fn(web::assets::serve_versioned));
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
        .layer(middleware::from_fn(security_headers))
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
/// `http://<Host>`.
///
/// Beyond legacy, any other state-changing request that the browser marks as coming from another
/// site (`Sec-Fetch-Site: cross-site` or `same-site`) is refused too, unless it carries a bearer
/// token. Without that, a page elsewhere could send a body-less `fetch(…, {method: 'POST',
/// credentials: 'include'})` — a "simple" request that needs no CORS preflight — and the session
/// cookie would ride along from a sibling subdomain (`SameSite=Lax` treats it as same-site). The
/// native clients send a bearer token and no `Sec-Fetch-Site`, so they are not affected; going by
/// that header rather than `Origin` keeps a proxy without `ORIGIN` working for the web UI's own
/// `fetch()` calls.
async fn reject_cross_site_forms(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let unsafe_method = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if unsafe_method {
        let headers = request.headers();
        if is_form(headers)
            && let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        {
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
        if from_another_site(headers) && !has_bearer(headers) {
            return detail_response(
                StatusCode::FORBIDDEN,
                "Anfragen von fremden Seiten sind nicht erlaubt",
            );
        }
    }
    next.run(request).await
}

fn from_another_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|site| {
            site.eq_ignore_ascii_case("cross-site") || site.eq_ignore_ascii_case("same-site")
        })
}

fn has_bearer(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(' ').next())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("bearer"))
}

/// Everything the web UI loads comes from this server, except artwork from the podcast directory
/// and feeds (`https:` images). Inline `style` attributes carry cover URLs and layout variables,
/// so styles stay `'unsafe-inline'`; scripts do not — the UI has no inline script or handler.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; media-src 'self' blob:; \
    connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; \
    frame-ancestors 'none'";

/// Browser hardening on every response: no framing (clickjacking on the settings pages), no
/// content sniffing (a stored cover or episode served as something else), no full URLs in the
/// `Referer` to podcast hosts, and the CSP above. Headers a handler already set are kept.
async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "same-origin"),
        (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
    ] {
        if !headers.contains_key(&name) {
            headers.insert(name, header::HeaderValue::from_static(value));
        }
    }
    response
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
