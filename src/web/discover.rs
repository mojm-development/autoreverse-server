//! `/library/discover`: AI suggestions and missing albums, and `/library/discover/mix` for one
//! of the mixes. Both pages are shells; static/js/discover.js fills them from the JSON API.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};
use serde::Deserialize;

use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::WebResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/discover", get(page))
        .route("/library/discover/mix", get(mix_page))
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
        ai_enabled: state.settings.get().ai().is_some(),
    };
    Ok(Html(page.render()?))
}

/// A mix has no id of its own: it lives in the stored feed only, as an entry of a list that the
/// next run replaces. It is found by its name, the way the apps tell mixes apart (`mix:<name>`).
#[derive(Deserialize)]
struct MixQuery {
    #[serde(default)]
    name: String,
}

#[derive(Template)]
#[template(path = "pages/library/mix.html")]
struct MixPage {
    shell: Shell,
    ai_enabled: bool,
    name: String,
}

async fn mix_page(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Query(query): Query<MixQuery>,
) -> WebResult<Html<String>> {
    let shell = Shell::load(&state, &user, &uri).await?;
    let page = MixPage {
        shell,
        ai_enabled: state.settings.get().ai().is_some(),
        name: query.name,
    };
    Ok(Html(page.render()?))
}
