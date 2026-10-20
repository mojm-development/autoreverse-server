//! `/settings`: the overview, Bibliotheken (library roots), Scan, Wiedergabe and Über (legacy
//! `routes/settings/**`). Nutzer and Sicherheit live in `web/users.rs`, Podcast-Abos in
//! `web/podcasts.rs`.

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get, routing::post};

use super::layout::Shell;
use super::scan_card::ScanCard;
use crate::api::scan::{cancel_scan, start_scan};
use crate::auth::preferences::{
    Preferences, SKIP_MAX, SKIP_MIN, SPEED_MAX, SPEED_MIN, set_preferences,
};
use crate::auth::{WebAdmin, WebUser};
use crate::error::WebResult;
use crate::library::paths::{LibraryPaths, get_library_paths, set_library_paths};
use crate::library::queries::{LibraryCounts, count_missing, delete_missing, library_counts};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(overview))
        .route("/settings/libraries", get(libraries).post(libraries_action))
        .route("/settings/scan", get(scan_page))
        .route("/settings/scan/card", get(scan_card))
        .route("/settings/scan/start", post(scan_start))
        .route("/settings/scan/cancel", post(scan_cancel))
        .route("/settings/playback", get(playback).post(playback_action))
        .route("/settings/about", get(about))
}

pub struct Section {
    pub href: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub admin: bool,
}

/// The settings sections (legacy `settingsSections.ts`).
pub const SECTIONS: &[Section] = &[
    Section {
        href: "/settings/libraries",
        label: "Bibliotheken",
        description: "Pfade zu den Hörbuch- und Musikordnern festlegen.",
        admin: true,
    },
    Section {
        href: "/settings/scan",
        label: "Scan",
        description: "Bibliothek neu einlesen und den Fortschritt verfolgen.",
        admin: true,
    },
    Section {
        href: "/settings/duplicates",
        label: "Doppelte Alben",
        description: "Mehrfach vorhandene Alben finden und überzählige Kopien ausblenden.",
        admin: true,
    },
    Section {
        href: "/settings/users",
        label: "Nutzer",
        description: "Konten anlegen und Verwalterrechte vergeben.",
        admin: true,
    },
    Section {
        href: "/settings/playback",
        label: "Wiedergabe",
        description: "Geschwindigkeit und Sprungweiten für die Wiedergabe anpassen.",
        admin: false,
    },
    Section {
        href: "/settings/podcasts",
        label: "Podcast-Abos",
        description: "Feeds abonnieren, aktualisieren und wieder entfernen.",
        admin: true,
    },
    Section {
        href: "/settings/security",
        label: "Sicherheit",
        description: "Eigenes Passwort ändern.",
        admin: false,
    },
    Section {
        href: "/settings/about",
        label: "Über Autoreverse",
        description: "Version und Projektinformationen.",
        admin: false,
    },
];

/// The shell around every settings page: the sidebar (books accent, "/settings" active) and the
/// sections the subnav and the overview list — only those the user may open.
pub(crate) async fn settings_shell(
    state: &AppState,
    user: &WebUser,
    uri: &Uri,
) -> WebResult<(Shell, Vec<&'static Section>)> {
    let mut shell = Shell::load(&state.db, user, uri).await?;
    shell.accent = "book";
    shell.active_href = "/settings".into();
    shell.active_query = String::new();
    let sections = SECTIONS
        .iter()
        .filter(|s| !s.admin || shell.is_admin)
        .collect();
    Ok((shell, sections))
}

pub(crate) fn web_user(admin: &WebAdmin) -> WebUser {
    WebUser {
        id: admin.id,
        token: admin.token.clone(),
    }
}

#[derive(Template)]
#[template(path = "pages/settings/overview.html")]
struct OverviewPage {
    shell: Shell,
    sections: Vec<&'static Section>,
}

async fn overview(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    let (shell, sections) = settings_shell(&state, &user, &uri).await?;
    Ok(Html(OverviewPage { shell, sections }.render()?))
}

#[derive(Template)]
#[template(path = "pages/settings/libraries.html")]
struct LibrariesPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    paths: LibraryPaths,
    missing: i64,
    error: Option<&'static str>,
    card: ScanCard,
}

async fn render_libraries(
    state: &AppState,
    admin: &WebAdmin,
    uri: &Uri,
    error: Option<&'static str>,
) -> WebResult<Html<String>> {
    let (shell, sections) = settings_shell(state, &web_user(admin), uri).await?;
    let (paths, missing) =
        tokio::try_join!(get_library_paths(&state.db), count_missing(&state.db, None))?;
    let page = LibrariesPage {
        shell,
        sections,
        paths,
        missing,
        error,
        card: ScanCard::from(&state.scan.snapshot()),
    };
    Ok(Html(page.render()?))
}

async fn libraries(
    State(state): State<AppState>,
    admin: WebAdmin,
    uri: Uri,
) -> WebResult<Html<String>> {
    render_libraries(&state, &admin, &uri, None).await
}

/// The page's two forms, told apart like SvelteKit's named actions: `?/savePaths` and
/// `?/cleanupMissing`. Either way the page renders again with the outcome.
async fn libraries_action(
    State(state): State<AppState>,
    admin: WebAdmin,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> WebResult<Html<String>> {
    let uri: Uri = "/settings/libraries".parse().expect("static uri");
    match query.as_deref() {
        Some("/cleanupMissing") => {
            delete_missing(&state.db).await?;
            render_libraries(&state, &admin, &uri, None).await
        }
        _ => {
            let form: HashMap<String, String> =
                serde_urlencoded::from_bytes(&body).unwrap_or_default();
            let field = |key: &str| form.get(key).map(|v| v.trim()).unwrap_or("");
            let (books_dir, music_dir) = (field("booksDir"), field("musicDir"));
            if books_dir.is_empty() || music_dir.is_empty() {
                return render_libraries(
                    &state,
                    &admin,
                    &uri,
                    Some("Beide Pfade werden benötigt."),
                )
                .await;
            }
            set_library_paths(&state.db, books_dir, music_dir).await?;
            render_libraries(&state, &admin, &uri, None).await
        }
    }
}

#[derive(Template)]
#[template(path = "pages/settings/playback.html")]
struct PlaybackPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    prefs: Preferences,
    speed_min: f64,
    speed_max: f64,
    skip_min: i32,
    skip_max: i32,
    saved: bool,
    error: Option<&'static str>,
}

async fn render_playback(
    state: &AppState,
    user: &WebUser,
    uri: &Uri,
    saved: bool,
    error: Option<&'static str>,
) -> WebResult<Html<String>> {
    // The shell carries the stored preferences already (for the player); the form shows them.
    let (shell, sections) = settings_shell(state, user, uri).await?;
    let page = PlaybackPage {
        prefs: shell.preferences,
        shell,
        sections,
        speed_min: SPEED_MIN,
        speed_max: SPEED_MAX,
        skip_min: SKIP_MIN,
        skip_max: SKIP_MAX,
        saved,
        error,
    };
    Ok(Html(page.render()?))
}

/// Settings → Wiedergabe: speed and skip distances, for every user.
async fn playback(
    State(state): State<AppState>,
    user: WebUser,
    uri: Uri,
) -> WebResult<Html<String>> {
    render_playback(&state, &user, &uri, false, None).await
}

/// A form number the way `Number()` reads it: an empty field is 0. `None` if it is no number.
fn form_number(form: &HashMap<String, String>, key: &str) -> Option<f64> {
    let raw = form.get(key).map_or("", |v| v.trim());
    if raw.is_empty() {
        return Some(0.0);
    }
    raw.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Stores the form's values, clamped into their bounds (legacy's default action), and shows
/// the page again with "Gespeichert".
async fn playback_action(
    State(state): State<AppState>,
    user: WebUser,
    body: Bytes,
) -> WebResult<Html<String>> {
    let uri: Uri = "/settings/playback".parse().expect("static uri");
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let values = (
        form_number(&form, "playbackSpeed"),
        form_number(&form, "skipBack"),
        form_number(&form, "skipForward"),
    );
    let (Some(speed), Some(back), Some(forward)) = values else {
        return render_playback(
            &state,
            &user,
            &uri,
            false,
            Some("Bitte nur Zahlen eingeben."),
        )
        .await;
    };
    set_preferences(&state.db, user.id, speed, back, forward).await?;
    render_playback(&state, &user, &uri, true, None).await
}

#[derive(Template)]
#[template(path = "pages/settings/about.html")]
struct AboutPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    version: &'static str,
    counts: LibraryCounts,
    /// Admins only, like `user_count` in `GET /server/info`.
    user_count: Option<i64>,
    data_dir: Option<String>,
}

/// Settings → Über: version and what `/server/info` knows, for every user.
async fn about(State(state): State<AppState>, user: WebUser, uri: Uri) -> WebResult<Html<String>> {
    let (shell, sections) = settings_shell(&state, &user, &uri).await?;
    let counts = library_counts(&state.db).await?;
    let (user_count, data_dir) = if shell.is_admin {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&state.db)
            .await?;
        (Some(n), Some(state.config.data_dir.display().to_string()))
    } else {
        (None, None)
    };
    let page = AboutPage {
        shell,
        sections,
        version: crate::VERSION,
        counts,
        user_count,
        data_dir,
    };
    Ok(Html(page.render()?))
}

#[derive(Template)]
#[template(path = "pages/settings/scan.html")]
struct ScanPage {
    shell: Shell,
    sections: Vec<&'static Section>,
    card: ScanCard,
}

async fn scan_page(
    State(state): State<AppState>,
    admin: WebAdmin,
    uri: Uri,
) -> WebResult<Html<String>> {
    let (shell, sections) = settings_shell(&state, &web_user(&admin), &uri).await?;
    let card = ScanCard::from(&state.scan.snapshot());
    Ok(Html(
        ScanPage {
            shell,
            sections,
            card,
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "partials/scan_card.html")]
struct ScanCardPartial {
    card: ScanCard,
}

fn render_card(state: &AppState) -> WebResult<Html<String>> {
    let card = ScanCard::from(&state.scan.snapshot());
    Ok(Html(ScanCardPartial { card }.render()?))
}

async fn scan_card(State(state): State<AppState>, _admin: WebAdmin) -> WebResult<Html<String>> {
    render_card(&state)
}

/// Starts a scan; a 409 (already running) changes nothing the card would not show anyway.
async fn scan_start(State(state): State<AppState>, _admin: WebAdmin) -> WebResult<Html<String>> {
    let _ = start_scan(&state);
    render_card(&state)
}

async fn scan_cancel(State(state): State<AppState>, _admin: WebAdmin) -> WebResult<Html<String>> {
    let _ = cancel_scan(&state);
    render_card(&state)
}
