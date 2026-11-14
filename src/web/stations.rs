//! `/library/radio`: the user's internet radio stations as tiles, the directory search and the
//! forms behind the page — add (from a search hit or by address), rename, remove. Each form
//! answers with a redirect back to the page, which htmx follows inside `#page`, so a station
//! that is playing keeps playing. Playing itself is static/js/player (`data-action
//! ="play-station"`); the search is rendered here rather than fetched by the browser, so the
//! browser never talks to the directory or loads a logo from somebody else's server — the
//! logos of the hits come through `GET /radio/stations/search/logo`.

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get, routing::post};

use super::layout::{RADIO_HREF, Shell};
use crate::api::stations::{RADIO_OFF, UNKNOWN_STATION};
use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::state::AppState;
use crate::stations::add::{self, AddError, NAME_INVALID, valid_name};
use crate::stations::directory::{self, DEFAULT_LIMIT, DirectoryStation};
use crate::stations::logos;
use crate::stations::store::{self, Station};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/radio", get(page).post(create))
        .route("/library/radio/{id}/rename", post(rename))
        .route("/library/radio/{id}/delete", post(delete))
}

/// "DE · MP3 · 192 kbit/s", as far as it is known.
fn facts(country: Option<&str>, codec: Option<&str>, bitrate: Option<i32>) -> String {
    let bitrate = bitrate.map(|kbps| format!("{kbps} kbit/s"));
    [country, codec, bitrate.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

pub struct StationTile {
    pub id: i32,
    pub name: String,
    pub subtitle: String,
    pub has_logo: bool,
}

impl From<Station> for StationTile {
    fn from(station: Station) -> Self {
        StationTile {
            id: station.id,
            subtitle: facts(
                station.country.as_deref(),
                station.codec.as_deref(),
                station.bitrate,
            ),
            has_logo: station.logo_file.is_some(),
            name: station.name,
        }
    }
}

pub struct SearchHit {
    pub directory_id: String,
    pub name: String,
    pub facts: String,
    pub tags: String,
    /// Whether a logo is worth asking for (`DirectoryStation::may_have_logo`); the image itself
    /// may still turn out a 404.
    pub has_logo: bool,
    pub added: bool,
}

/// What the "add by address" form held when it was refused, to show it again.
#[derive(Default)]
struct Entered {
    name: String,
    stream_url: String,
}

#[derive(Template)]
#[template(path = "pages/library/radio.html")]
struct RadioPage {
    shell: Shell,
    stations: Vec<StationTile>,
    /// Whether the server may ask the directory (`AUTOREVERSE_RADIO_DIRECTORY`).
    directory: bool,
    query: String,
    /// `None` until something was searched for.
    hits: Option<Vec<SearchHit>>,
    search_error: Option<String>,
    error: Option<String>,
    entered: Entered,
}

/// 404 inside the app frame on a server without the feature.
fn guard(state: &AppState) -> WebResult<()> {
    if state.settings.get().radio {
        Ok(())
    } else {
        Err(WebError::not_found(RADIO_OFF))
    }
}

async fn search(
    state: &AppState,
    user: &WebUser,
    query: &str,
) -> WebResult<Result<Vec<SearchHit>, String>> {
    let config = &state.config;
    let found = directory::search(
        &state.http,
        &config.radio_directory_url,
        query,
        None,
        DEFAULT_LIMIT,
    )
    .await;
    let found: Vec<DirectoryStation> = match found {
        Ok(found) => found,
        Err(e) => return Ok(Err(e.0)),
    };
    state.radio_logos.remember(&found);
    let mine = store::directory_ids(&state.db, user.id).await?;
    Ok(Ok(found
        .into_iter()
        .map(|station| SearchHit {
            added: mine
                .iter()
                .any(|id| id.eq_ignore_ascii_case(&station.directory_id)),
            facts: facts(
                station.country.as_deref(),
                station.codec.as_deref(),
                station.bitrate,
            ),
            tags: station.tags.join(", "),
            has_logo: station.may_have_logo(),
            directory_id: station.directory_id,
            name: station.name,
        })
        .collect()))
}

async fn render(
    state: &AppState,
    user: &WebUser,
    query: &str,
    error: Option<String>,
    entered: Entered,
) -> WebResult<Html<String>> {
    let uri: Uri = RADIO_HREF.parse().expect("static uri");
    let (shell, stations) = tokio::try_join!(Shell::load(state, user, &uri), async {
        Ok(store::list(&state.db, user.id).await?)
    })?;
    logos::backfill(state, &stations);
    let directory = state.settings.get().radio_directory_on();
    let query: String = query.trim().chars().take(100).collect();
    let (hits, search_error) = if directory && !query.is_empty() {
        match search(state, user, &query).await? {
            Ok(hits) => (Some(hits), None),
            Err(message) => (None, Some(message)),
        }
    } else {
        (None, None)
    };
    let page = RadioPage {
        shell,
        stations: stations.into_iter().map(StationTile::from).collect(),
        directory,
        query,
        hits,
        search_error,
        error,
        entered,
    };
    Ok(Html(page.render()?))
}

/// The page; `?q=` searches the directory.
async fn page(
    State(state): State<AppState>,
    user: WebUser,
    Query(params): Query<HashMap<String, String>>,
) -> WebResult<Html<String>> {
    guard(&state)?;
    let query = params.get("q").map_or("", String::as_str);
    render(&state, &user, query, None, Entered::default()).await
}

/// Back to the page, keeping the search that was open.
fn back(query: &str) -> Response {
    let query = query.trim();
    if query.is_empty() {
        return Redirect::to(RADIO_HREF).into_response();
    }
    let encoded = serde_urlencoded::to_string([("q", query)]).unwrap_or_default();
    Redirect::to(&format!("{RADIO_HREF}?{encoded}")).into_response()
}

/// "Hinzufügen": a search hit (`directory_id`) or an address with a name. A station that cannot
/// be added comes back as the page with the reason, the form still filled in.
async fn create(State(state): State<AppState>, user: WebUser, body: Bytes) -> WebResult<Response> {
    guard(&state)?;
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let field = |key: &str| form.get(key).map_or("", |value| value.trim());
    let query = field("q");
    let by_address = field("directory_id").is_empty();
    let added = if by_address {
        add::add_by_address(&state, user.id, field("stream_url"), field("name"), None).await
    } else {
        add::add_from_directory(&state, user.id, field("directory_id")).await
    };
    let (status, message) = match added {
        Ok(_) => return Ok(back(query)),
        Err(AddError::Db(e)) => return Err(e.into()),
        Err(e @ AddError::Invalid(_)) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        Err(e @ (AddError::DirectoryOff | AddError::UnknownStation)) => {
            (StatusCode::NOT_FOUND, e.to_string())
        }
        Err(e @ AddError::Directory(_)) => (StatusCode::BAD_GATEWAY, e.to_string()),
    };
    let entered = if by_address {
        Entered {
            name: field("name").to_owned(),
            stream_url: field("stream_url").to_owned(),
        }
    } else {
        Entered::default()
    };
    let page = render(&state, &user, query, Some(message), entered).await?;
    Ok((status, page).into_response())
}

async fn own(state: &AppState, user: &WebUser, raw_id: &str) -> WebResult<Station> {
    guard(state)?;
    let not_found = || WebError::not_found(UNKNOWN_STATION);
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    store::owned(&state.db, user.id, id)
        .await?
        .ok_or_else(not_found)
}

async fn rename(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> WebResult<Response> {
    let station = own(&state, &user, &raw_id).await?;
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let Some(name) = form.get("name").and_then(|name| valid_name(name)) else {
        let page = render(
            &state,
            &user,
            "",
            Some(NAME_INVALID.into()),
            Entered::default(),
        )
        .await?;
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response());
    };
    store::rename(&state.db, station.id, name).await?;
    Ok(back(""))
}

/// "Entfernen". A stream of the station that is running ends when its listener stops; nothing
/// else is kept of it.
async fn delete(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let station = own(&state, &user, &raw_id).await?;
    if let Some(removed) = store::remove(&state.db, user.id, station.id).await? {
        logos::remove(&state.config.radio_dir, &removed).await;
    }
    Ok(back(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_list_what_is_known() {
        assert_eq!(
            facts(Some("DE"), Some("MP3"), Some(192)),
            "DE · MP3 · 192 kbit/s"
        );
        assert_eq!(facts(None, Some("AAC"), None), "AAC");
        assert_eq!(facts(None, None, None), "");
    }
}
