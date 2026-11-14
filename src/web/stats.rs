//! `/library/stats` ("Statistik": what was heard most) and `/library/review` ("Rückblick": a
//! calendar year). New in Rust. Both render what `library::top` reads out of the listening
//! ledger — the same numbers `GET /me/top` and `GET /me/review` give the apps.

use std::collections::HashSet;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get};
use chrono::{Datelike, Utc};
use sqlx::PgPool;

use super::format::{encode_component, human_duration, initials};
use super::layout::Shell;
use crate::auth::WebUser;
use crate::error::WebResult;
use crate::library::queries::Item;
use crate::library::top::{
    MAX_YEAR, MIN_YEAR, Period, Range, TopAlbum, TopArtist, TopGenre, TopTrack, review,
    review_years, top_albums, top_artists, top_genres, top_tracks,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/stats", get(stats))
        .route("/library/review", get(year_review))
}

/// How many entries each list of the statistics page shows.
const STATS_TOP: i64 = 10;

/// The share of the list's first entry, for the bar behind a row: 4–100.
fn share(seconds: f64, most: f64) -> u32 {
    if most <= 0.0 {
        return 0;
    }
    ((seconds / most * 100.0).round() as u32).clamp(4, 100)
}

/// "3 Hörvorgänge" is no word anybody says: "3×".
fn plays_label(plays: i32) -> String {
    format!("{plays}×")
}

pub struct TrackRow {
    pub id: i32,
    pub item_id: i32,
    pub title: String,
    pub byline: String,
    pub time: String,
    pub plays: String,
    pub share: u32,
}

pub struct AlbumRow {
    pub id: i32,
    pub title: String,
    pub artist: String,
    pub has_cover: bool,
    pub time: String,
    pub plays: String,
    pub share: u32,
}

pub struct ArtistRow {
    pub name: String,
    pub initials: String,
    /// The artist's page, for those who have one: album artists with a present album.
    pub href: Option<String>,
    pub image: Option<String>,
    pub time: String,
    pub plays: String,
    pub share: u32,
}

pub struct GenreRow {
    pub name: String,
    pub href: String,
    pub time: String,
    pub share: u32,
}

/// The four lists, ready to draw. Shared by both pages (`partials/top_lists.html`).
#[derive(Default)]
pub struct TopLists {
    pub tracks: Vec<TrackRow>,
    pub albums: Vec<AlbumRow>,
    pub artists: Vec<ArtistRow>,
    pub genres: Vec<GenreRow>,
}

impl TopLists {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.genres.is_empty()
    }
}

/// Which of `names` have an artist page: one statement for the whole list.
async fn with_page(db: &PgPool, names: &[String]) -> Result<HashSet<String>, sqlx::Error> {
    let found: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT artist FROM items
         WHERE kind = 'album' AND artist = ANY($1) AND missing_since IS NULL
           AND duplicate_of IS NULL",
    )
    .bind(names)
    .fetch_all(db)
    .await?;
    Ok(found.into_iter().collect())
}

async fn top_lists(
    db: &PgPool,
    tracks: &[TopTrack],
    albums: &[TopAlbum],
    artists: &[TopArtist],
    genres: &[TopGenre],
) -> Result<TopLists, sqlx::Error> {
    let names: Vec<String> = artists.iter().map(|a| a.artist.clone()).collect();
    let pages = if names.is_empty() {
        HashSet::new()
    } else {
        with_page(db, &names).await?
    };
    let most = |seconds: Option<f64>| seconds.unwrap_or(0.0);
    let (most_track, most_album, most_artist, most_genre) = (
        most(tracks.first().map(|t| t.seconds)),
        most(albums.first().map(|a| a.seconds)),
        most(artists.first().map(|a| a.seconds)),
        most(genres.first().map(|g| g.seconds)),
    );
    Ok(TopLists {
        tracks: tracks
            .iter()
            .map(|top| TrackRow {
                id: top.track.id,
                item_id: top.track.item_id,
                title: top.track.title.clone().unwrap_or_default(),
                byline: top
                    .track
                    .artist
                    .clone()
                    .unwrap_or_else(|| top.track.item_title.clone()),
                time: human_duration(top.seconds),
                plays: plays_label(top.plays),
                share: share(top.seconds, most_track),
            })
            .collect(),
        albums: albums
            .iter()
            .map(|top| AlbumRow {
                id: top.item.id,
                title: top.item.title.clone(),
                artist: top.item.artist.clone().unwrap_or_default(),
                has_cover: top.item.cover_path.is_some(),
                time: human_duration(top.seconds),
                plays: plays_label(top.plays),
                share: share(top.seconds, most_album),
            })
            .collect(),
        artists: artists
            .iter()
            .map(|top| {
                let encoded = encode_component(&top.artist);
                ArtistRow {
                    initials: initials(&top.artist),
                    href: pages
                        .contains(&top.artist)
                        .then(|| format!("/library/artists/{encoded}")),
                    image: top
                        .has_image
                        .then(|| format!("/artists/{encoded}/image?size=128")),
                    name: top.artist.clone(),
                    time: human_duration(top.seconds),
                    plays: plays_label(top.plays),
                    share: share(top.seconds, most_artist),
                }
            })
            .collect(),
        genres: genres
            .iter()
            .map(|top| GenreRow {
                href: format!("/library/albums?genre={}", encode_component(&top.genre)),
                name: top.genre.clone(),
                time: human_duration(top.seconds),
                share: share(top.seconds, most_genre),
            })
            .collect(),
    })
}

pub struct PeriodChip {
    pub href: String,
    pub label: &'static str,
    pub current: bool,
}

#[derive(Template)]
#[template(path = "pages/library/stats.html")]
struct StatsPage {
    shell: Shell,
    periods: Vec<PeriodChip>,
    lists: TopLists,
    /// The year the link to the review names.
    year: i32,
}

#[derive(serde::Deserialize)]
struct StatsQuery {
    period: Option<String>,
}

/// `?period=week|month|year|all`; anything else is the last 30 days, as in the API.
async fn stats(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Query(query): Query<StatsQuery>,
) -> WebResult<Html<String>> {
    let period = query
        .period
        .as_deref()
        .and_then(Period::parse)
        .unwrap_or(Period::Month);
    let range = Range::last(period, Utc::now());
    let db = &state.db;
    let (shell, tracks, albums, artists, genres) = tokio::try_join!(
        Shell::load(&state, &user, &uri),
        async { Ok(top_tracks(db, user.id, &range, STATS_TOP).await?) },
        async { Ok(top_albums(db, user.id, &range, STATS_TOP).await?) },
        async { Ok(top_artists(db, user.id, &range, STATS_TOP).await?) },
        async { Ok(top_genres(db, user.id, &range, STATS_TOP).await?) },
    )?;
    let page = StatsPage {
        shell,
        periods: Period::ALL
            .into_iter()
            .map(|p| PeriodChip {
                href: format!("/library/stats?period={}", p.key()),
                label: p.label(),
                current: p == period,
            })
            .collect(),
        lists: top_lists(db, &tracks, &albums, &artists, &genres).await?,
        year: Utc::now().year(),
    };
    Ok(Html(page.render()?))
}

pub struct YearChip {
    pub year: i32,
    pub current: bool,
}

/// One figure of the review with what it counts.
pub struct Fact {
    pub value: String,
    pub label: &'static str,
    pub detail: String,
}

pub struct MonthBar {
    pub label: &'static str,
    /// Height of the bar in percent of the year's fullest month.
    pub height: u32,
    pub time: String,
}

pub struct KindShare {
    pub label: &'static str,
    pub accent: &'static str,
    pub time: String,
    /// Width in percent of the year's total.
    pub percent: u32,
}

pub struct BookTile {
    pub id: i32,
    pub title: String,
    pub author: String,
    pub has_cover: bool,
}

impl From<&Item> for BookTile {
    fn from(item: &Item) -> Self {
        BookTile {
            id: item.id,
            title: item.title.clone(),
            author: item.author.clone().unwrap_or_default(),
            has_cover: item.cover_path.is_some(),
        }
    }
}

pub struct FirstListen {
    pub href: String,
    pub title: String,
    pub day: String,
}

#[derive(Template)]
#[template(path = "pages/library/review.html")]
struct ReviewPage {
    shell: Shell,
    year: i32,
    years: Vec<YearChip>,
    /// Nothing was heard in this year.
    empty: bool,
    /// Whole hours of the year, the page's headline figure.
    hours: String,
    kinds: Vec<KindShare>,
    facts: Vec<Fact>,
    months: Vec<MonthBar>,
    lists: TopLists,
    books: Vec<BookTile>,
    first: Option<FirstListen>,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];

/// `31.01.`: a day of the year the page is about.
fn day_label(day: chrono::NaiveDate) -> String {
    day.format("%d.%m.").to_string()
}

/// "1 Tag", "12 Tage".
fn days_label(days: i32) -> String {
    if days == 1 {
        "1 Tag".into()
    } else {
        format!("{days} Tage")
    }
}

fn item_href(item: &Item) -> String {
    let segment = match item.kind.as_str() {
        "book" => "books",
        "album" => "albums",
        // An episode's page is its podcast's.
        _ => {
            return format!("/library/podcasts/{}", item.parent_id.unwrap_or(item.id));
        }
    };
    format!("/library/{segment}/{}", item.id)
}

#[derive(serde::Deserialize)]
struct ReviewQuery {
    year: Option<String>,
}

/// `?year=`; without one, or with one that is none, the current year.
async fn year_review(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
    Query(query): Query<ReviewQuery>,
) -> WebResult<Html<String>> {
    let this_year = Utc::now().year();
    let year = query
        .year
        .as_deref()
        .and_then(|raw| raw.trim().parse::<i32>().ok())
        .filter(|year| (MIN_YEAR..=MAX_YEAR).contains(year))
        .unwrap_or(this_year);
    let db = &state.db;
    let (shell, found, mut known_years) = tokio::try_join!(
        Shell::load(&state, &user, &uri),
        async { Ok(review(db, user.id, year).await?) },
        async { Ok(review_years(db, user.id).await?) },
    )?;
    // The year shown and the current one are always on offer, also before anything was heard.
    for extra in [year, this_year] {
        if !known_years.contains(&extra) {
            known_years.push(extra);
        }
    }
    known_years.sort_unstable_by(|a, b| b.cmp(a));

    let totals = &found.totals;
    let percent = |seconds: f64| {
        if totals.total > 0.0 {
            (seconds / totals.total * 100.0).round() as u32
        } else {
            0
        }
    };
    let kinds = [
        ("Musik", "music", totals.music),
        ("Hörbücher", "book", totals.books),
        ("Podcasts", "podcast", totals.podcasts),
    ]
    .into_iter()
    .filter(|(_, _, seconds)| *seconds > 0.0)
    .map(|(label, accent, seconds)| KindShare {
        label,
        accent,
        time: human_duration(seconds),
        percent: percent(seconds),
    })
    .collect();
    let mut facts = vec![Fact {
        value: totals.days_listened.to_string(),
        label: "Tage gehört",
        detail: String::new(),
    }];
    if let Some((day, seconds)) = totals.longest_day {
        facts.push(Fact {
            value: human_duration(seconds),
            label: "Längster Tag",
            detail: day_label(day),
        });
    }
    facts.push(Fact {
        value: days_label(totals.longest_streak_days),
        label: "Längste Serie",
        detail: "am Stück gehört".into(),
    });
    facts.push(Fact {
        value: found.new_artists.to_string(),
        label: "Neue Interpreten",
        detail: "zum ersten Mal gehört".into(),
    });
    if !found.books_finished.is_empty() {
        facts.push(Fact {
            value: found.books_finished.len().to_string(),
            label: "Hörbücher beendet",
            detail: String::new(),
        });
    }
    let fullest = totals.months.iter().copied().fold(0.0, f64::max);
    let months = MONTHS
        .iter()
        .zip(totals.months)
        .map(|(label, seconds)| MonthBar {
            label,
            height: if seconds > 0.0 {
                share(seconds, fullest)
            } else {
                0
            },
            time: human_duration(seconds),
        })
        .collect();

    let page = ReviewPage {
        shell,
        year,
        years: known_years
            .into_iter()
            .map(|y| YearChip {
                year: y,
                current: y == year,
            })
            .collect(),
        empty: totals.total <= 0.0,
        hours: format!("{}", (totals.total / 3600.0).round() as i64),
        kinds,
        facts,
        months,
        lists: top_lists(
            db,
            &found.top_tracks,
            &found.top_albums,
            &found.top_artists,
            &found.top_genres,
        )
        .await?,
        books: found.books_finished.iter().map(BookTile::from).collect(),
        first: found.first_listen.as_ref().map(|(at, item)| FirstListen {
            href: item_href(item),
            title: item.title.clone(),
            day: at.format("%d.%m.%Y").to_string(),
        }),
    };
    Ok(Html(page.render()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_is_a_share_of_the_longest_and_never_invisible() {
        assert_eq!(share(50.0, 100.0), 50);
        assert_eq!(share(100.0, 100.0), 100);
        assert_eq!(share(0.5, 100.0), 4);
        assert_eq!(share(10.0, 0.0), 0);
    }

    #[test]
    fn days_are_counted_in_german() {
        assert_eq!(days_label(0), "0 Tage");
        assert_eq!(days_label(1), "1 Tag");
        assert_eq!(days_label(12), "12 Tage");
    }
}
