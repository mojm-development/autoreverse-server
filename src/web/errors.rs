//! Error pages inside the app shell (legacy `library/+error.svelte` and `settings/+error.svelte`).
//!
//! SvelteKit rendered an error under /library or /settings in the nearest layout, so the sidebar
//! (and the settings subnav) stayed; only a logged-out visitor or the fullscreen player got the
//! standalone page. Handlers here return [`WebError`] without the shell at hand, so this
//! middleware re-renders the standalone page's response in the shell once the handler is done:
//! [`error_page`](crate::error::error_page) marks its responses with [`ErrorPageInfo`].

use askama::Template;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};

use super::layout::Shell;
use super::settings::{Section, settings_shell};
use crate::auth::WebUser;
use crate::state::AppState;

/// What an error page showed; set as a response extension by `error_page`.
#[derive(Debug, Clone)]
pub struct ErrorPageInfo {
    pub status: StatusCode,
    pub message: String,
}

#[derive(Template)]
#[template(path = "pages/error_library.html")]
struct LibraryErrorPage<'a> {
    shell: Shell,
    status: u16,
    message: &'a str,
}

#[derive(Template)]
#[template(path = "pages/error_settings.html")]
struct SettingsErrorPage<'a> {
    shell: Shell,
    sections: Vec<&'static Section>,
    status: u16,
    message: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Area {
    Library,
    Settings,
}

/// Which layout's error page a path gets: /library and /settings have their own, the fullscreen
/// player (`/library/{id}/player`) sits outside the library layout like in legacy.
fn area(path: &str) -> Option<Area> {
    let under = |prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
    if under("/settings") {
        Some(Area::Settings)
    } else if under("/library") && !path.ends_with("/player") {
        Some(Area::Library)
    } else {
        None
    }
}

pub async fn shell_errors(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(area) = area(request.uri().path()) else {
        return next.run(request).await;
    };
    if request.method() != Method::GET {
        return next.run(request).await;
    }
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let response = next.run(request).await;
    let Some(info) = response.extensions().get::<ErrorPageInfo>().cloned() else {
        return response;
    };
    let mut probe = Request::new(());
    *probe.headers_mut() = headers;
    let (mut parts, ()) = probe.into_parts();
    let Ok(user) = WebUser::from_request_parts(&mut parts, &state).await else {
        return response;
    };
    match render(&state, &user, &uri, area, &info).await {
        Some(html) => (info.status, Html(html)).into_response(),
        None => response,
    }
}

async fn render(
    state: &AppState,
    user: &WebUser,
    uri: &Uri,
    area: Area,
    info: &ErrorPageInfo,
) -> Option<String> {
    let status = info.status.as_u16();
    let message = info.message.as_str();
    let rendered = match area {
        Area::Library => {
            let shell = Shell::load(&state.db, user, uri).await.ok()?;
            LibraryErrorPage {
                shell,
                status,
                message,
            }
            .render()
        }
        Area::Settings => {
            let (shell, sections) = settings_shell(state, user, uri).await.ok()?;
            SettingsErrorPage {
                shell,
                sections,
                status,
                message,
            }
            .render()
        }
    };
    rendered
        .map_err(|e| tracing::error!("rendering the error page in the shell failed: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_and_settings_errors_keep_their_layout_the_player_does_not() {
        assert_eq!(area("/library/books/9"), Some(Area::Library));
        assert_eq!(area("/library"), Some(Area::Library));
        assert_eq!(area("/settings/users"), Some(Area::Settings));
        assert_eq!(area("/library/9/player"), None);
        assert_eq!(area("/libraryx"), None);
        assert_eq!(area("/items/9"), None);
    }
}
