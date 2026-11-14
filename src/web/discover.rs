//! `/library/discover`: AI suggestions and missing albums. The page is a shell; static/js/discover.js
//! fills it from the JSON API.

use askama::Template;
use axum::extract::State;
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::WebResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/library/discover", get(page))
}

#[derive(Template)]
#[template(path = "pages/library/discover.html")]
struct DiscoverPage {
    shell: Shell,
    ai_enabled: bool,
}

async fn page(State(state): State<AppState>, user: WebUser, uri: Uri) -> WebResult<Html<String>> {
    let shell = Shell::load(&state, &user, &uri).await?;
    let page = DiscoverPage {
        shell,
        ai_enabled: state.config.ai.is_some(),
    };
    Ok(Html(page.render()?))
}
