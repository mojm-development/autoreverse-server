//! `/library/albums` and `/library/albums/{id}` (legacy `library/albums/**`). Playing and the
//! hearts are static/js/player and app.js, editing (Verwalter) static/js/metadata.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::books::Link;
use super::format::{encode_component, format_hours, human_duration};
use super::layout::Shell;
use super::metadata::{BulkEditor, MetadataEditor};
use super::playlists::{PlaylistChoice, playlist_choices};
use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::library::favorites::{favorite_track_ids, is_item_favorite};
use crate::library::queries::{
    self, Item, ItemsFilter, PAGE_SIZE, SortKey, albums_of_artist, count_items, count_missing,
    item_durations, tracks,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/albums", get(list))
        // Static beats the `{id}` capture: the infinite scroll's next page.
        .route("/library/albums/more", get(more))
        .route("/library/albums/{id}", get(detail))
}

const ALBUM_SORTS: [SortKey; 2] = [SortKey::Title, SortKey::Added];

/// One album in the grid or the list.
pub struct AlbumRow {
    /// 1-based place in the whole list, also across pages.
    pub number: i64,
    pub id: i32,
    pub title: String,
    pub artist: String,
    pub has_cover: bool,
    pub year: String,
    pub duration: String,
}

fn album_rows(rows: &[Item], durations: &HashMap<i32, f64>, first: i64) -> Vec<AlbumRow> {
    rows.iter()
        .enumerate()
        .map(|(i, album)| {
            let duration = human_duration(durations.get(&album.id).copied().unwrap_or(0.0));
            AlbumRow {
                number: first + i as i64,
                id: album.id,
                title: album.title.clone(),
                artist: album.artist.clone().unwrap_or_default(),
                has_cover: album.cover_path.is_some(),
                year: album.year.map_or_else(|| "—".into(), |y| y.to_string()),
                duration: if duration.is_empty() {
                    "—".into()
                } else {
                    duration
                },
            }
        })
        .collect()
}

/// The list's parameters, so a link can change one and keep the rest.
struct ListParams {
    q: String,
    artist: Option<String>,
    missing: bool,
    sort: SortKey,
    view: &'static str,
}

impl ListParams {
    fn from_query(query: &HashMap<String, String>) -> ListParams {
        let get = |key: &str| query.get(key).filter(|v| !v.is_empty()).cloned();
        ListParams {
            q: get("q").unwrap_or_default(),
            artist: get("artist"),
            missing: get("missing").as_deref() == Some("true"),
            sort: get("sort")
                .and_then(|s| SortKey::parse(&s))
                .filter(|s| ALBUM_SORTS.contains(s))
                .unwrap_or(SortKey::Title),
            view: if get("view").as_deref() == Some("list") {
                "list"
            } else {
                "grid"
            },
        }
    }

    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut params = Vec::new();
        if !self.q.is_empty() {
            params.push(("q", self.q.clone()));
        }
        if let Some(artist) = &self.artist {
            params.push(("artist", artist.clone()));
        }
        if self.missing {
            params.push(("missing", "true".into()));
        }
        if self.sort != SortKey::Title {
            params.push(("sort", self.sort.key().into()));
        }
        if self.view != "grid" {
            params.push(("view", self.view.into()));
        }
        params
    }

    fn href_with(&self, base: &str, overrides: &[(&'static str, &str)]) -> String {
        let mut params = self.pairs();
        // `URLSearchParams.set`: a present key keeps its place, a new one goes last.
        for (key, value) in overrides {
            if value.is_empty() {
                params.retain(|(k, _)| k != key);
            } else if let Some(entry) = params.iter_mut().find(|(k, _)| k == key) {
                entry.1 = (*value).to_owned();
            } else {
                params.push((key, (*value).to_owned()));
            }
        }
        let query = params
            .iter()
            .map(|(k, v)| format!("{k}={}", encode_component(v)))
            .collect::<Vec<_>>()
            .join("&");
        if query.is_empty() {
            base.to_owned()
        } else {
            format!("{base}?{query}")
        }
    }

    fn href(&self, overrides: &[(&'static str, &str)]) -> String {
        self.href_with("/library/albums", overrides)
    }

    /// The next page for the infinite scroll, or `None` when this one was the last.
    fn more_href(&self, loaded: usize, offset: i64) -> Option<String> {
        (self.artist.is_none() && loaded as i64 == PAGE_SIZE).then(|| {
            let next = (offset + PAGE_SIZE).to_string();
            self.href_with("/library/albums/more", &[("offset", &next)])
        })
    }

    fn heading(&self) -> String {
        match (&self.artist, self.missing) {
            (Some(artist), _) => format!("Alben von {artist}"),
            (None, true) => "Fehlende Alben".into(),
            (None, false) => "Alben".into(),
        }
    }
}

/// One page of albums, in title or added order, or present-only unless `missing`.
async fn page_of(
    state: &AppState,
    params: &ListParams,
    offset: i64,
) -> Result<Vec<Item>, sqlx::Error> {
    queries::items(
        &state.db,
        &ItemsFilter {
            kind: Some("album".into()),
            q: (!params.q.is_empty()).then(|| params.q.clone()),
            missing: Some(params.missing),
            sort: Some(params.sort),
            limit: PAGE_SIZE,
            offset,
            ..Default::default()
        },
    )
    .await
}

#[derive(Template)]
#[template(path = "pages/library/albums.html")]
struct AlbumsPage {
    shell: Shell,
    heading: String,
    view: &'static str,
    q: String,
    artist: Option<String>,
    missing: bool,
    sort: &'static str,
    total: i64,
    albums: Vec<AlbumRow>,
    sort_options: Vec<Link>,
    grid_href: String,
    list_href: String,
    clear_q_href: String,
    clear_artist_href: String,
    clear_missing_href: String,
    more_href: Option<String>,
    empty: String,
    pickable: bool,
    bulk: Option<BulkEditor>,
}

async fn list(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> WebResult<Html<String>> {
    let shell = Shell::load(&state.db, &user, &uri).await?;
    let params = ListParams::from_query(&query);
    let (rows, total) = match &params.artist {
        Some(artist) => {
            let rows = albums_of_artist(&state.db, artist).await?;
            let total = rows.len() as i64;
            (rows, total)
        }
        None => {
            let q = (!params.q.is_empty()).then_some(params.q.as_str());
            tokio::try_join!(page_of(&state, &params, 0), async {
                if params.missing {
                    count_missing(&state.db, Some("album")).await
                } else {
                    count_items(&state.db, "album", q).await
                }
            })?
        }
    };
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let durations = item_durations(&state.db, &ids).await?;
    let sort_options = ALBUM_SORTS
        .iter()
        .map(|key| Link {
            label: key.label().into(),
            href: params.href(&[("sort", key.key())]),
            current: *key == params.sort,
        })
        .collect();
    let empty = if params.q.is_empty() {
        "Noch keine Alben in der Bibliothek.".into()
    } else {
        format!("Nichts gefunden für „{}“.", params.q)
    };
    let pickable = shell.is_admin;
    let bulk = pickable.then(|| {
        BulkEditor::new(
            "album",
            total,
            Some(BulkEditor::album_filter(
                &params.q,
                params.artist.as_deref(),
                params.missing,
            )),
        )
    });
    let bulk = match bulk {
        Some(bulk) => Some(bulk.with_last(&state.db, user.id).await?),
        None => None,
    };
    let page = AlbumsPage {
        pickable,
        bulk,
        shell,
        heading: params.heading(),
        view: params.view,
        sort: params.sort.key(),
        total,
        more_href: params.more_href(rows.len(), 0),
        albums: album_rows(&rows, &durations, 1),
        sort_options,
        grid_href: params.href(&[("view", "grid")]),
        list_href: params.href(&[("view", "list")]),
        clear_q_href: params.href(&[("q", "")]),
        clear_artist_href: params.href(&[("artist", "")]),
        clear_missing_href: params.href(&[("missing", "")]),
        empty,
        q: params.q.clone(),
        artist: params.artist.clone(),
        missing: params.missing,
    };
    Ok(Html(page.render()?))
}

#[derive(Template)]
#[template(path = "partials/album_page.html")]
struct AlbumsChunk {
    pickable: bool,
    view: &'static str,
    albums: Vec<AlbumRow>,
    more_href: Option<String>,
}

/// The next page of the list as bare tiles or rows plus the next sentinel (legacy
/// `InfiniteScroll` + `/library/more`), swapped in by htmx in place of the old sentinel.
async fn more(
    State(state): State<AppState>,
    user: WebUser,
    Query(query): Query<HashMap<String, String>>,
) -> WebResult<Html<String>> {
    let pickable = is_admin(&state, user.id).await?;
    let params = ListParams::from_query(&query);
    let offset = query
        .get("offset")
        .and_then(|o| o.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    let rows = if params.artist.is_some() {
        Vec::new()
    } else {
        page_of(&state, &params, offset).await?
    };
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let durations = item_durations(&state.db, &ids).await?;
    let chunk = AlbumsChunk {
        pickable,
        view: params.view,
        more_href: params.more_href(rows.len(), offset),
        albums: album_rows(&rows, &durations, offset + 1),
    };
    Ok(Html(chunk.render()?))
}

pub struct TrackRow {
    pub id: i32,
    pub number: usize,
    pub title: String,
    pub duration: String,
    pub favorite: bool,
}

#[derive(Template)]
#[template(path = "pages/library/album.html")]
struct AlbumPage {
    shell: Shell,
    album: Item,
    eyebrow: String,
    subline: String,
    artist: String,
    tracks: Vec<TrackRow>,
    favorite: bool,
    playlists: Vec<PlaylistChoice>,
    /// For Verwalter.
    editor: Option<MetadataEditor>,
}

async fn is_admin(state: &AppState, user_id: i32) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT is_admin FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?
            .unwrap_or(false),
    )
}

async fn detail(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Path(raw_id): Path<String>,
) -> WebResult<Html<String>> {
    let not_found = || WebError::not_found("Unbekanntes Album");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let album = queries::item(&state.db, id)
        .await?
        .filter(|a| a.kind == "album")
        .ok_or_else(not_found)?;
    let (shell, track_rows, favorite, favorite_tracks, playlists) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        async { Ok(tracks(&state.db, album.id).await?) },
        async { Ok(is_item_favorite(&state.db, user.id, album.id).await?) },
        async { Ok::<_, WebError>(favorite_track_ids(&state.db, user.id, album.id).await?) },
        playlist_choices(&state.db, user.id),
    )?;
    let total: f64 = track_rows.iter().map(|t| t.duration).sum();
    let eyebrow = match album.year {
        Some(year) => format!("Album · {year}"),
        None => "Album".into(),
    };
    let subline = format!(
        "{} · {} Titel · {}",
        album.artist.as_deref().unwrap_or(""),
        track_rows.len(),
        format_hours(total)
    );
    let editor = shell
        .is_admin
        .then(|| MetadataEditor::new(&album, &track_rows, "Titel"));
    let page = AlbumPage {
        editor,
        shell,
        eyebrow,
        subline,
        artist: album.artist.clone().unwrap_or_else(|| "—".into()),
        tracks: track_rows
            .iter()
            .enumerate()
            .map(|(i, t)| TrackRow {
                id: t.id,
                number: i + 1,
                title: t
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Titel {}", i + 1)),
                duration: format_hours(t.duration),
                favorite: favorite_tracks.contains(&t.id),
            })
            .collect(),
        favorite,
        playlists,
        album,
    };
    Ok(Html(page.render()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> ListParams {
        ListParams::from_query(
            &pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn links_keep_the_other_parameters() {
        let p = params(&[("q", "Nord licht"), ("view", "list")]);
        assert_eq!(
            p.href(&[("sort", "added")]),
            "/library/albums?q=Nord%20licht&view=list&sort=added"
        );
        assert_eq!(p.href(&[("q", "")]), "/library/albums?view=list");
    }

    #[test]
    fn only_title_and_added_sorts_apply() {
        assert_eq!(params(&[("sort", "added")]).sort, SortKey::Added);
        assert_eq!(params(&[("sort", "series")]).sort, SortKey::Title);
        assert_eq!(params(&[("sort", "quatsch")]).sort, SortKey::Title);
    }

    #[test]
    fn a_full_page_links_the_next_one_unless_filtered_by_artist() {
        let p = params(&[("sort", "added")]);
        assert_eq!(
            p.more_href(PAGE_SIZE as usize, 100).as_deref(),
            Some("/library/albums/more?sort=added&offset=200")
        );
        assert_eq!(p.more_href(5, 0), None);
        assert_eq!(
            params(&[("artist", "X")]).more_href(PAGE_SIZE as usize, 0),
            None
        );
    }

    #[test]
    fn the_heading_names_the_filter() {
        assert_eq!(params(&[]).heading(), "Alben");
        assert_eq!(params(&[("missing", "true")]).heading(), "Fehlende Alben");
        assert_eq!(
            params(&[("artist", "Kraftwerk")]).heading(),
            "Alben von Kraftwerk"
        );
    }
}
