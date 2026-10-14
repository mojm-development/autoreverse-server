//! `/library/podcasts` (Abos and "Neue Folgen"), `/library/podcasts/preview`,
//! `/library/podcasts/{id}` and Settings → Podcast-Abos (legacy `library/podcasts/**`,
//! `settings/podcasts`, `PodcastRail`, `PodcastSearch`, `EpisodeCard`).
//!
//! Subscribing, refreshing, unsubscribing and "Folgen vorhalten" are forms that answer with a
//! redirect (htmx follows it inside `#page`, so the player keeps playing). The directory search,
//! "Abonnieren" in its results, downloads and playing go to the JSON API from static/js.

use std::collections::{HashMap, HashSet};

use askama::Template;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get, routing::post};
use chrono::Local;

use super::format::{encode_component, format_hours, human_duration, numeric_date, relative_day};
use super::layout::Shell;
use super::settings::{Section, settings_shell, web_user};
use crate::api::podcasts::{UNKNOWN_PODCAST, retain_in_background};
use crate::auth::{WebAdmin, WebUser};
use crate::error::{WebError, WebResult};
use crate::library::queries::{
    self, Item, PodcastOverview, item_durations, item_durations_with_feed_fallback, new_episodes,
    podcast_overview, progress_map,
};
use crate::podcasts::feed::ParsedFeed;
use crate::podcasts::retention::{
    KEEP_MAX, get_keep_default, retain_for_podcast, set_keep_default, set_keep_for_podcast,
};
use crate::podcasts::store::{FeedError, preview_feed, refresh, subscribe, unsubscribe};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/podcasts", get(overview))
        .route("/library/podcasts/subscribe", post(subscribe_form))
        .route(
            "/library/podcasts/preview",
            get(preview).post(preview_subscribe),
        )
        .route("/library/podcasts/{id}", get(detail))
        .route("/library/podcasts/{id}/keep", post(keep))
        .route("/library/podcasts/{id}/refresh", post(refresh_form))
        .route("/library/podcasts/{id}/unsubscribe", post(unsubscribe_form))
        .route("/settings/podcasts", get(settings).post(settings_keep))
}

pub const UNREACHABLE: &str = "Der Feed ist nicht erreichbar.";
pub const UNREADABLE: &str = "Der Feed konnte nicht gelesen werden.";

/// The German line for a feed that failed; anything else is a server error.
fn feed_problem(e: FeedError) -> WebResult<&'static str> {
    match e {
        FeedError::Fetch(_) => Ok(UNREACHABLE),
        FeedError::Invalid(_) => Ok(UNREADABLE),
        FeedError::NotFound => Err(WebError::not_found(UNKNOWN_PODCAST)),
        FeedError::Db(e) => Err(e.into()),
    }
}

fn form(body: &Bytes) -> HashMap<String, String> {
    serde_urlencoded::from_bytes(body).unwrap_or_default()
}

fn query_map(raw: Option<&str>) -> HashMap<String, String> {
    serde_urlencoded::from_str(raw.unwrap_or("")).unwrap_or_default()
}

/// One subscription in the rail (legacy `PodcastRail.svelte`).
pub struct RailEntry {
    pub id: i32,
    pub title: String,
    pub has_cover: bool,
    pub episode_count: i32,
    pub unheard_count: i32,
}

impl From<PodcastOverview> for RailEntry {
    fn from(p: PodcastOverview) -> Self {
        RailEntry {
            id: p.id,
            title: p.title,
            has_cover: p.cover_path.is_some(),
            episode_count: p.episode_count,
            unheard_count: p.unheard_count,
        }
    }
}

/// The rail of every podcast page: all subscriptions, the open one marked.
pub struct Rail {
    pub entries: Vec<RailEntry>,
    pub active_id: Option<i32>,
    /// The "Feed-URL hinzufügen" form below the list (detail page, Verwalter only).
    pub add_form: bool,
}

impl Rail {
    pub fn is_active(&self, id: &i32) -> bool {
        self.active_id == Some(*id)
    }
}

async fn load_rail(state: &AppState, user_id: i32) -> WebResult<Rail> {
    Ok(Rail {
        entries: podcast_overview(&state.db, user_id)
            .await?
            .into_iter()
            .map(RailEntry::from)
            .collect(),
        active_id: None,
        add_form: false,
    })
}

/// One episode (legacy `EpisodeCard.svelte`).
pub struct EpisodeCard {
    pub id: i32,
    pub title: String,
    pub date: String,
    pub duration: String,
    pub downloaded: bool,
    pub finished: bool,
    pub started: bool,
    pub percent: String,
    pub left: String,
    /// Artwork URL; without one the card shows `index` instead (or nothing).
    pub cover_url: Option<String>,
    pub index: Option<usize>,
    pub show: Option<(String, String)>,
    /// `unheard`, `started` or `finished`, for the filter pills.
    pub status: &'static str,
}

impl EpisodeCard {
    pub fn play_label(&self) -> String {
        format!(
            "{} {}",
            self.title,
            if self.started {
                "fortsetzen"
            } else {
                "abspielen"
            }
        )
    }

    pub fn play_title(&self) -> &'static str {
        if self.started {
            "Fortsetzen"
        } else {
            "Abspielen"
        }
    }
}

struct CardInput {
    id: i32,
    title: String,
    published_at: Option<chrono::DateTime<chrono::Utc>>,
    duration: f64,
    downloaded: bool,
    position: f64,
    finished: bool,
    cover_url: Option<String>,
    index: Option<usize>,
    show: Option<(String, String)>,
}

fn status_of(started: Option<bool>) -> &'static str {
    match started {
        None => "unheard",
        Some(false) => "started",
        Some(true) => "finished",
    }
}

fn card(input: CardInput, status: &'static str) -> EpisodeCard {
    let started = !input.finished && input.position > 0.0;
    let percent = if input.duration > 0.0 {
        (input.position / input.duration * 100.0).min(100.0)
    } else {
        0.0
    };
    EpisodeCard {
        id: input.id,
        title: input.title,
        date: relative_day(input.published_at, Local::now()),
        duration: human_duration(input.duration),
        downloaded: input.downloaded,
        finished: input.finished,
        started,
        percent: format!("{}", (percent * 100.0).round() / 100.0),
        left: human_duration((input.duration - input.position).max(0.0)),
        cover_url: input.cover_url,
        index: input.index,
        show: input.show,
        status,
    }
}

/// The directory search box (legacy `PodcastSearch.svelte`), filled in by static/js/podcasts.js.
pub struct Search {
    pub query: String,
}

#[derive(Template)]
#[template(path = "pages/library/podcasts.html")]
struct OverviewPage {
    shell: Shell,
    rail: Rail,
    show_new: bool,
    episodes: Vec<EpisodeCard>,
    new_subline: String,
    search: Search,
    error: Option<&'static str>,
}

async fn render_overview(
    state: &AppState,
    user: &WebUser,
    uri: &Uri,
    query: &str,
    show_new: bool,
    error: Option<&'static str>,
) -> WebResult<Html<String>> {
    let (shell, rail) =
        tokio::try_join!(Shell::load(&state.db, user, uri), load_rail(state, user.id))?;
    let rows = if show_new {
        new_episodes(&state.db, user.id, 60).await?
    } else {
        Vec::new()
    };
    let shows: HashSet<i32> = rows.iter().map(|e| e.podcast_id).collect();
    let new_subline = if rows.is_empty() {
        "Nichts Ungehörtes".to_owned()
    } else {
        format!(
            "{} {} aus {} {}",
            rows.len(),
            if rows.len() == 1 { "Folge" } else { "Folgen" },
            shows.len(),
            if shows.len() == 1 { "Abo" } else { "Abos" }
        )
    };
    let episodes = rows
        .into_iter()
        .map(|e| {
            // An episode rarely carries its own artwork; fall back to the show's.
            let cover_url = if e.cover_path.is_some() {
                Some(format!("/items/{}/cover", e.id))
            } else if e.podcast_has_cover {
                Some(format!("/items/{}/cover", e.podcast_id))
            } else {
                None
            };
            card(
                CardInput {
                    id: e.id,
                    title: e.title,
                    published_at: e.published_at,
                    duration: e.duration,
                    downloaded: e.downloaded,
                    position: e.position,
                    finished: false,
                    cover_url,
                    index: None,
                    show: Some((
                        e.podcast_title,
                        format!("/library/podcasts/{}", e.podcast_id),
                    )),
                },
                if e.position > 0.0 {
                    "started"
                } else {
                    "unheard"
                },
            )
        })
        .collect();
    let page = OverviewPage {
        shell,
        rail,
        show_new,
        episodes,
        new_subline,
        search: Search {
            query: query.to_owned(),
        },
        error,
    };
    Ok(Html(page.render()?))
}

async fn overview(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let query = query_map(uri.query());
    let show_new = query.get("filter").is_some_and(|f| f == "new");
    let q = query.get("q").cloned().unwrap_or_default();
    render_overview(&state, &user, &uri, &q, show_new, None).await
}

/// "Abonnieren" in the rail of a podcast page: subscribe, then on to the new podcast.
async fn subscribe_form(
    State(state): State<AppState>,
    admin: WebAdmin,
    body: Bytes,
) -> WebResult<Response> {
    let feed_url = form(&body)
        .get("feed_url")
        .map(|u| u.trim().to_owned())
        .unwrap_or_default();
    let uri: Uri = "/library/podcasts".parse().expect("static uri");
    let user = web_user(&admin);
    if feed_url.is_empty() {
        return Ok(
            render_overview(&state, &user, &uri, "", false, Some(UNREACHABLE))
                .await?
                .into_response(),
        );
    }
    match subscribe(
        &state.db,
        &state.http,
        &feed_url,
        Some(&state.config.cover_dir),
    )
    .await
    {
        Ok(synced) => {
            retain_in_background(&state, synced.id);
            Ok(Redirect::to(&format!("/library/podcasts/{}", synced.id)).into_response())
        }
        Err(e) => {
            let problem = feed_problem(e)?;
            Ok(
                render_overview(&state, &user, &uri, "", false, Some(problem))
                    .await?
                    .into_response(),
            )
        }
    }
}

/// `feed` must be an http(s) URL; it comes back normalised.
fn valid_feed_url(raw: Option<&String>) -> WebResult<String> {
    let raw = raw.filter(|r| !r.is_empty()).ok_or_else(|| {
        WebError::Page(
            StatusCode::BAD_REQUEST,
            "Keine Feed-Adresse angegeben".into(),
        )
    })?;
    let invalid = || WebError::Page(StatusCode::BAD_REQUEST, "Ungültige Feed-Adresse".into());
    let url = reqwest::Url::parse(raw).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid());
    }
    Ok(url.to_string())
}

pub struct PreviewEpisode {
    pub title: String,
    pub date: String,
    pub duration: String,
    pub description: Option<String>,
}

pub struct PreviewFeed {
    pub title: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub episode_count: usize,
    pub episodes: Vec<PreviewEpisode>,
}

impl PreviewFeed {
    /// The background for the artwork tile, with the characters that would end `url("…")`
    /// escaped.
    pub fn cover_style(&self) -> String {
        self.image_url.as_deref().map_or_else(String::new, |url| {
            format!(
                "background-image: url(\"{}\")",
                url.replace('\\', "%5C").replace('"', "%22")
            )
        })
    }

    pub fn more(&self) -> usize {
        self.episode_count - self.episodes.len()
    }
}

/// How many episodes the preview page lists.
const PREVIEW_PAGE_EPISODES: usize = 30;

impl From<ParsedFeed> for PreviewFeed {
    fn from(feed: ParsedFeed) -> Self {
        PreviewFeed {
            episode_count: feed.episodes.len(),
            episodes: feed
                .episodes
                .into_iter()
                .take(PREVIEW_PAGE_EPISODES)
                .map(|e| PreviewEpisode {
                    title: e.title,
                    date: numeric_date(e.published_at),
                    duration: e
                        .duration_seconds
                        .filter(|d| *d > 0.0)
                        .map(format_hours)
                        .unwrap_or_default(),
                    description: e.description,
                })
                .collect(),
            title: feed.title,
            description: feed.description,
            image_url: feed.image_url,
        }
    }
}

#[derive(Template)]
#[template(path = "pages/library/podcast_preview.html")]
struct PreviewPage {
    shell: Shell,
    rail: Rail,
    feed_url: String,
    feed_param: String,
    back_href: String,
    subscribed_id: Option<i32>,
    feed: Option<PreviewFeed>,
    load_error: Option<&'static str>,
    error: Option<&'static str>,
}

async fn subscribed_id(state: &AppState, feed_url: &str) -> WebResult<Option<i32>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM items WHERE kind = 'podcast' AND feed_url = $1")
            .bind(feed_url)
            .fetch_optional(&state.db)
            .await?,
    )
}

async fn render_preview(
    state: &AppState,
    admin: &WebAdmin,
    query: &HashMap<String, String>,
    error: Option<&'static str>,
) -> WebResult<Html<String>> {
    let feed_url = valid_feed_url(query.get("feed"))?;
    let q = query.get("q").cloned().unwrap_or_default();
    let user = web_user(admin);
    let uri: Uri = "/library/podcasts/preview".parse().expect("static uri");
    let (shell, rail, existing) = tokio::try_join!(
        Shell::load(&state.db, &user, &uri),
        load_rail(state, user.id),
        subscribed_id(state, &feed_url)
    )?;
    let (feed, load_error) = match preview_feed(&state.http, &feed_url).await {
        Ok(parsed) => (Some(PreviewFeed::from(parsed)), None),
        Err(e) => (None, Some(feed_problem(e)?)),
    };
    let back_href = if q.is_empty() {
        "/library/podcasts".to_owned()
    } else {
        format!("/library/podcasts?q={}", encode_component(&q))
    };
    let mut feed_param = encode_component(&feed_url);
    if !q.is_empty() {
        feed_param.push_str(&format!("&q={}", encode_component(&q)));
    }
    let page = PreviewPage {
        shell,
        rail,
        feed_url,
        feed_param,
        back_href,
        subscribed_id: existing,
        feed,
        load_error,
        error,
    };
    Ok(Html(page.render()?))
}

async fn preview(
    State(state): State<AppState>,
    admin: WebAdmin,
    RawQuery(raw): RawQuery,
) -> WebResult<Html<String>> {
    render_preview(&state, &admin, &query_map(raw.as_deref()), None).await
}

/// "Abonnieren" on the preview page.
async fn preview_subscribe(
    State(state): State<AppState>,
    admin: WebAdmin,
    RawQuery(raw): RawQuery,
) -> WebResult<Response> {
    let query = query_map(raw.as_deref());
    let feed_url = valid_feed_url(query.get("feed"))?;
    match subscribe(
        &state.db,
        &state.http,
        &feed_url,
        Some(&state.config.cover_dir),
    )
    .await
    {
        Ok(synced) => {
            retain_in_background(&state, synced.id);
            Ok(Redirect::to(&format!("/library/podcasts/{}", synced.id)).into_response())
        }
        Err(e) => {
            let problem = feed_problem(e)?;
            Ok(render_preview(&state, &admin, &query, Some(problem))
                .await?
                .into_response())
        }
    }
}

/// The episode to pick up: the newest one started, else the newest unheard one.
pub struct Continue {
    pub id: i32,
    pub downloaded: bool,
    pub started: bool,
}

#[derive(Template)]
#[template(path = "pages/library/podcast.html")]
struct DetailPage {
    shell: Shell,
    rail: Rail,
    podcast: Item,
    episodes: Vec<EpisodeCard>,
    unheard_count: usize,
    started_count: usize,
    downloaded_count: usize,
    last_checked: String,
    continue_with: Option<Continue>,
    keep_default: i32,
    keep_max: i32,
    keep_value: String,
    error: Option<String>,
}

impl DetailPage {
    pub fn filters(&self) -> Vec<(&'static str, &'static str, usize)> {
        vec![
            ("all", "Alle", self.episodes.len()),
            ("unheard", "Ungehört", self.unheard_count),
            ("started", "Angefangen", self.started_count),
            ("downloaded", "Geladen", self.downloaded_count),
        ]
    }

    pub fn play_label(&self) -> &'static str {
        match &self.continue_with {
            Some(c) if c.started => "Fortsetzen",
            _ => "Neueste abspielen",
        }
    }
}

async fn find_podcast(state: &AppState, raw_id: &str) -> WebResult<Item> {
    let not_found = || WebError::not_found(UNKNOWN_PODCAST);
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let item = queries::item(&state.db, id).await?.ok_or_else(not_found)?;
    if item.kind != "podcast" {
        return Err(not_found());
    }
    Ok(item)
}

async fn render_detail(
    state: &AppState,
    user: &WebUser,
    podcast: Item,
    error: Option<String>,
) -> WebResult<Html<String>> {
    let uri: Uri = format!("/library/podcasts/{}", podcast.id)
        .parse()
        .expect("podcast uri");
    let (shell, mut rail, rows, keep_default) = tokio::try_join!(
        Shell::load(&state.db, user, &uri),
        load_rail(state, user.id),
        async { Ok(queries::children(&state.db, podcast.id).await?) },
        async { Ok(get_keep_default(&state.db).await?) }
    )?;
    rail.active_id = Some(podcast.id);
    rail.add_form = shell.is_admin;
    let ids: Vec<i32> = rows.iter().map(|e| e.id).collect();
    let (downloaded, durations, progress) = tokio::try_join!(
        item_durations(&state.db, &ids),
        item_durations_with_feed_fallback(&state.db, &ids),
        progress_map(&state.db, user.id, &ids)
    )?;
    let total = rows.len();
    let episodes: Vec<EpisodeCard> = rows
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let p = progress.get(&e.id);
            card(
                CardInput {
                    id: e.id,
                    duration: durations.get(&e.id).copied().unwrap_or(0.0),
                    downloaded: downloaded.contains_key(&e.id),
                    position: p.map_or(0.0, |p| p.position),
                    finished: p.is_some_and(|p| p.finished),
                    published_at: e.published_at,
                    title: e.title,
                    cover_url: None,
                    // Feed order is newest first, so the oldest episode is number one.
                    index: Some(total - i),
                    show: None,
                },
                status_of(p.map(|p| p.finished)),
            )
        })
        .collect();
    let count = |status: &str| episodes.iter().filter(|e| e.status == status).count();
    let continue_with = episodes
        .iter()
        .find(|e| e.status == "started")
        .or_else(|| episodes.iter().find(|e| e.status == "unheard"))
        .or_else(|| episodes.first())
        .map(|e| Continue {
            id: e.id,
            downloaded: e.downloaded,
            started: e.status == "started",
        });
    let last_checked = match relative_day(podcast.last_checked, Local::now()) {
        label if label.is_empty() => "nie".to_owned(),
        label => label.to_lowercase(),
    };
    let page = DetailPage {
        unheard_count: count("unheard"),
        started_count: count("started"),
        downloaded_count: episodes.iter().filter(|e| e.downloaded).count(),
        shell,
        rail,
        keep_value: podcast
            .keep_episodes
            .map(|k| k.to_string())
            .unwrap_or_default(),
        podcast,
        episodes,
        last_checked,
        continue_with,
        keep_default,
        keep_max: KEEP_MAX,
        error,
    };
    Ok(Html(page.render()?))
}

/// The podcast page. An episode id (search hits, favourites and bookmarks link episodes here)
/// leads to its podcast.
async fn detail(
    State(state): State<AppState>,
    user: WebUser,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let not_found = || WebError::not_found(UNKNOWN_PODCAST);
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let item = queries::item(&state.db, id).await?.ok_or_else(not_found)?;
    match (item.kind.as_str(), item.parent_id) {
        ("podcast", _) => Ok(render_detail(&state, &user, item, None)
            .await?
            .into_response()),
        ("episode", Some(parent)) => {
            Ok(Redirect::to(&format!("/library/podcasts/{parent}")).into_response())
        }
        _ => Err(not_found()),
    }
}

fn back_to(id: i32) -> Response {
    Redirect::to(&format!("/library/podcasts/{id}")).into_response()
}

/// A whole number the way `Number()` reads a form field; `None` if it is none.
fn whole_number(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let n = if raw.is_empty() {
        0.0
    } else {
        raw.parse::<f64>().ok()?
    };
    (n.is_finite() && n.fract() == 0.0).then_some(n)
}

fn keep_error() -> String {
    format!("Bitte eine Zahl zwischen 0 und {KEEP_MAX}")
}

/// "Folgen vorhalten" for one podcast; empty means the default.
async fn keep(
    State(state): State<AppState>,
    admin: WebAdmin,
    Path(raw_id): Path<String>,
    body: Bytes,
) -> WebResult<Response> {
    let podcast = find_podcast(&state, &raw_id).await?;
    let raw = form(&body).get("keep").cloned().unwrap_or_default();
    if raw.trim().is_empty() {
        set_keep_for_podcast(&state.db, podcast.id, None).await?;
        return Ok(back_to(podcast.id));
    }
    match whole_number(&raw).filter(|n| (0.0..=f64::from(KEEP_MAX)).contains(n)) {
        Some(n) => {
            set_keep_for_podcast(&state.db, podcast.id, Some(n)).await?;
            Ok(back_to(podcast.id))
        }
        None => Ok(
            render_detail(&state, &web_user(&admin), podcast, Some(keep_error()))
                .await?
                .into_response(),
        ),
    }
}

/// "Aktualisieren": fetch the feed, then download or prune per "Folgen vorhalten".
async fn refresh_form(
    State(state): State<AppState>,
    admin: WebAdmin,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let podcast = find_podcast(&state, &raw_id).await?;
    match refresh(
        &state.db,
        &state.http,
        podcast.id,
        Some(&state.config.cover_dir),
    )
    .await
    {
        Ok(_) => {
            retain_for_podcast(
                &state.db,
                &state.http,
                podcast.id,
                &state.config.podcasts_dir,
            )
            .await?;
            Ok(back_to(podcast.id))
        }
        Err(e) => {
            let problem = feed_problem(e)?;
            Ok(
                render_detail(&state, &web_user(&admin), podcast, Some(problem.to_owned()))
                    .await?
                    .into_response(),
            )
        }
    }
}

/// "Wirklich kündigen": the podcast, its episodes and their downloads go.
async fn unsubscribe_form(
    State(state): State<AppState>,
    _admin: WebAdmin,
    Path(raw_id): Path<String>,
) -> WebResult<Response> {
    let podcast = find_podcast(&state, &raw_id).await?;
    match unsubscribe(&state.db, podcast.id, &state.config.podcasts_dir).await {
        Ok(_) | Err(FeedError::NotFound) => Ok(Redirect::to("/library/podcasts").into_response()),
        Err(e) => Err(WebError::Internal(e.to_string())),
    }
}

#[derive(Template)]
#[template(path = "pages/settings/podcasts.html")]
struct SettingsPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    keep_default: i32,
    keep_max: i32,
    refresh_hint: String,
    search: Search,
    saved: bool,
    error: Option<String>,
}

/// What the page says about the automatic refresh.
pub fn refresh_hint(hours: f64) -> String {
    if hours == 1.0 {
        "Der Server aktualisiert alle Feeds jede Stunde von selbst.".into()
    } else if hours > 0.0 {
        format!("Der Server aktualisiert alle Feeds alle {hours} Stunden von selbst.")
    } else {
        "Die automatische Aktualisierung ist abgeschaltet (AUTOREVERSE_PODCAST_REFRESH_HOURS=0)."
            .into()
    }
}

async fn render_settings(
    state: &AppState,
    admin: &WebAdmin,
    saved: bool,
    error: Option<String>,
) -> WebResult<Html<String>> {
    let uri: Uri = "/settings/podcasts".parse().expect("static uri");
    let (shell, sections) = settings_shell(state, &web_user(admin), &uri).await?;
    let page = SettingsPage {
        shell,
        sections,
        keep_default: get_keep_default(&state.db).await?,
        keep_max: KEEP_MAX,
        refresh_hint: refresh_hint(state.config.podcast_refresh_hours),
        search: Search {
            query: String::new(),
        },
        saved,
        error,
    };
    Ok(Html(page.render()?))
}

async fn settings(State(state): State<AppState>, admin: WebAdmin) -> WebResult<Html<String>> {
    render_settings(&state, &admin, false, None).await
}

/// Settings → Podcast-Abos → "Folgen vorhalten": the default for every podcast.
async fn settings_keep(
    State(state): State<AppState>,
    admin: WebAdmin,
    body: Bytes,
) -> WebResult<Html<String>> {
    let raw = form(&body).get("keep").cloned().unwrap_or_default();
    match whole_number(&raw).filter(|n| (0.0..=f64::from(KEEP_MAX)).contains(n)) {
        Some(n) => {
            set_keep_default(&state.db, n).await?;
            render_settings(&state, &admin, true, None).await
        }
        None => render_settings(&state, &admin, false, Some(keep_error())).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refresh_hint_has_a_branch_for_one_hour() {
        assert!(refresh_hint(1.0).contains("jede Stunde"));
        assert!(refresh_hint(6.0).contains("alle 6 Stunden"));
        assert!(refresh_hint(0.0).contains("abgeschaltet"));
    }

    #[test]
    fn keep_numbers_are_whole_and_empty_is_zero() {
        assert_eq!(whole_number(" 3 "), Some(3.0));
        assert_eq!(whole_number(""), Some(0.0));
        assert_eq!(whole_number("2.5"), None);
        assert_eq!(whole_number("drei"), None);
    }

    #[test]
    fn a_card_shows_what_is_left_of_a_started_episode() {
        let c = card(
            CardInput {
                id: 1,
                title: "Angefangen".into(),
                published_at: None,
                duration: 3600.0,
                downloaded: false,
                position: 1800.0,
                finished: false,
                cover_url: None,
                index: None,
                show: None,
            },
            "started",
        );
        assert!(c.started);
        assert_eq!(c.percent, "50");
        assert_eq!(c.left, "30 min");
        assert_eq!(c.play_label(), "Angefangen fortsetzen");
    }

    #[test]
    fn feed_urls_must_be_http() {
        assert!(valid_feed_url(None).is_err());
        assert!(valid_feed_url(Some(&"file:///etc/passwd".to_string())).is_err());
        assert!(valid_feed_url(Some(&"kaputt".to_string())).is_err());
        assert_eq!(
            valid_feed_url(Some(&"https://x/feed.xml".to_string())).unwrap(),
            "https://x/feed.xml"
        );
    }
}
