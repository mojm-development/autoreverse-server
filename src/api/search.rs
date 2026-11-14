//! `GET /search?q=&limit=`: books, albums, podcasts (shows and episodes), tracks and — new in
//! Rust, as the web search has them — album artists and book authors matching `q`.

use axum::extract::State;
use axum::{Json, Router, routing::get};
use serde::Serialize;

use super::validate::{ApiQuery, QueryMap, int_param};
use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::queries::{search_artists, search_items, search_tracks};
use crate::serialize::{ItemSummary, TrackSummary};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search))
}

#[derive(Serialize)]
struct SearchOut {
    books: Vec<ItemSummary>,
    albums: Vec<ItemSummary>,
    podcasts: Vec<ItemSummary>,
    tracks: Vec<TrackSummary>,
    artists: Vec<NameOut>,
}

#[derive(Serialize)]
struct NameOut {
    name: String,
    /// `artist` (albums) or `author` (books).
    role: String,
    work_count: i32,
}

async fn search(
    State(state): State<AppState>,
    _user: ApiUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Json<SearchOut>> {
    let q = query.get("q").map(String::as_str).unwrap_or("");
    // JS `length`: UTF-16 code units.
    if !(1..=200).contains(&q.encode_utf16().count()) {
        return Err(ApiError::unprocessable("q muss 1–200 Zeichen haben"));
    }
    let limit = int_param(&query, "limit", 20, 1, 100)?;
    let (books, albums, podcasts, tracks, artists) = tokio::try_join!(
        search_items(&state.db, q, &["book"], limit),
        search_items(&state.db, q, &["album"], limit),
        search_items(&state.db, q, &["podcast", "episode"], limit),
        search_tracks(&state.db, q, limit),
        search_artists(&state.db, q, limit)
    )?;
    Ok(Json(SearchOut {
        books: books.iter().map(ItemSummary::from).collect(),
        albums: albums.iter().map(ItemSummary::from).collect(),
        podcasts: podcasts.iter().map(ItemSummary::from).collect(),
        tracks: tracks.iter().map(TrackSummary::from).collect(),
        artists: artists
            .into_iter()
            .map(|a| NameOut {
                name: a.name,
                role: a.role,
                work_count: a.work_count,
            })
            .collect(),
    }))
}
