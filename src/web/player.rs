//! `/library/{id}/player`: the fullscreen player (legacy `routes/(fullscreen)/library/[id]/player`).
//!
//! The server renders the item, its chapters or tracks and the bookmarks; static/js/player starts
//! the item if it is not already playing and keeps headline, scrubber and lists in step.

use askama::Template;
use axum::extract::{Path, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};

use super::books::{BookmarkRow, ChapterRow};
use super::format::format_hms;
use super::home::accent;
use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::{WebError, WebResult};
use crate::library::queries::{self, Item, bookmarks_for_item};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/library/{id}/player", get(player))
}

/// The speeds on offer (legacy `SPEEDS`).
const SPEEDS: [f64; 9] = [0.75, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

pub struct SpeedOption {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

/// `1,25×`: two decimals at least, German comma (`toLocaleString('de-DE')`).
pub fn speed_label(speed: f64) -> String {
    let fixed = format!("{speed:.2}");
    // More than two decimals only if the value has them (1.125 → 1,125×).
    let text = if (speed * 100.0).fract().abs() > 1e-9 {
        format!("{speed}")
    } else {
        fixed
    };
    format!("{}×", text.replace('.', ","))
}

pub struct TrackLine {
    pub id: i32,
    pub number: usize,
    pub title: String,
    pub length: String,
}

#[derive(Template)]
#[template(path = "pages/now_playing.html")]
struct PlayerPage {
    shell: Shell,
    item: Item,
    accent: &'static str,
    kind_label: &'static str,
    has_cover: bool,
    by_track: bool,
    byline: String,
    headline: String,
    subtitle: String,
    list_label: &'static str,
    step_back: i32,
    step_forward: i32,
    chapters: Vec<ChapterRow>,
    tracks: Vec<TrackLine>,
    bookmarks: Vec<BookmarkRow>,
    /// Chapter starts as percentages of the whole item.
    ticks: Vec<String>,
    speeds: Vec<SpeedOption>,
}

async fn player(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Path(raw_id): Path<String>,
) -> WebResult<Html<String>> {
    let not_found = || WebError::not_found("Unbekanntes Item");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let item = queries::item(&state.db, id).await?.ok_or_else(not_found)?;
    let (shell, chapter_rows, bookmark_rows, track_rows, cover) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        async { Ok(queries::chapters(&state.db, item.id).await?) },
        async { Ok(bookmarks_for_item(&state.db, user.id, item.id).await?) },
        async { Ok(queries::tracks(&state.db, item.id).await?) },
        // An episode inherits the show's artwork.
        async { Ok::<_, WebError>(queries::cover_path_for(&state.db, &item).await?) },
    )?;

    let by_track = item.kind == "album";
    let total: f64 = track_rows.iter().map(|t| t.duration).sum();
    // Not playing yet: position 0, as legacy showed before the player caught up.
    let chapters = ChapterRow::list(&chapter_rows, &item.title, 0.0);
    let (headline, subtitle) = if by_track {
        (item.title.clone(), String::new())
    } else {
        match chapters.first().filter(|c| c.end_secs > 0.0) {
            Some(first) => (
                first.label.clone(),
                format!("Kapitel 1 von {} · {}", chapters.len(), item.title),
            ),
            None => (item.title.clone(), String::new()),
        }
    };
    let prefs = shell.preferences;
    let (step_back, step_forward) = if by_track {
        (10, 10)
    } else {
        (prefs.skip_back, prefs.skip_forward)
    };
    let mut byline = item
        .author
        .clone()
        .or_else(|| item.artist.clone())
        .unwrap_or_default();
    if let Some(narrator) = &item.narrator {
        byline.push_str(&format!(" · {narrator}"));
    }
    let ticks = if by_track || total <= 0.0 {
        Vec::new()
    } else {
        chapter_rows
            .iter()
            .map(|c| format!("{:.3}", (c.start / total * 100.0).clamp(0.0, 100.0)))
            .collect()
    };

    let page = PlayerPage {
        accent: accent(&item.kind),
        kind_label: match item.kind.as_str() {
            "book" => "Hörbuch",
            "album" => "Album",
            _ => "Podcast",
        },
        has_cover: cover.is_some(),
        by_track,
        byline,
        headline,
        subtitle,
        list_label: if by_track { "Titel" } else { "Kapitel" },
        step_back,
        step_forward,
        chapters,
        tracks: track_rows
            .iter()
            .enumerate()
            .map(|(i, t)| TrackLine {
                id: t.id,
                number: i + 1,
                title: t
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Titel {}", i + 1)),
                length: format!("{} min", ((t.duration / 60.0).round() as i64).max(1)),
            })
            .collect(),
        bookmarks: bookmark_rows
            .iter()
            .map(|b| BookmarkRow {
                title: b.title.clone(),
                position: format_hms(b.position),
                position_secs: b.position,
            })
            .collect(),
        ticks,
        speeds: SPEEDS
            .iter()
            .map(|&value| SpeedOption {
                value: value.to_string(),
                label: speed_label(value),
                selected: (value - prefs.playback_speed).abs() < 1e-9,
            })
            .collect(),
        shell,
        item,
    };
    Ok(Html(page.render()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_read_like_to_locale_string_de() {
        assert_eq!(speed_label(1.0), "1,00×");
        assert_eq!(speed_label(1.25), "1,25×");
        assert_eq!(speed_label(1.1), "1,10×");
        assert_eq!(speed_label(0.75), "0,75×");
        assert_eq!(speed_label(1.125), "1,125×");
    }
}
