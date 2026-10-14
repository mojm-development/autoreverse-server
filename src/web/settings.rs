//! `/settings`: the overview, Bibliotheken (library roots) and Scan (legacy
//! `routes/settings/**`). The other sections follow with their slices; until then the overview
//! links to them anyway.

use std::collections::HashMap;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::Uri;
use axum::response::Html;
use axum::{Router, routing::get, routing::post};

use super::layout::Shell;
use crate::api::scan::{cancel_scan, start_scan};
use crate::auth::{WebAdmin, WebUser};
use crate::error::WebResult;
use crate::library::paths::{LibraryPaths, get_library_paths, set_library_paths};
use crate::library::queries::{count_missing, delete_missing};
use crate::scanner::state::{Phase, ScanState};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(overview))
        .route("/settings/libraries", get(libraries).post(libraries_action))
        .route("/settings/scan", get(scan_page))
        .route("/settings/scan/card", get(scan_card))
        .route("/settings/scan/start", post(scan_start))
        .route("/settings/scan/cancel", post(scan_cancel))
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
async fn settings_shell(
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

fn web_user(admin: &WebAdmin) -> WebUser {
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

/// Counts shown under the status line: live while running, else the last report.
pub struct Counts {
    pub new: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub removed: u32,
    pub skipped: u32,
}

/// What `partials/scan_card.html` shows (legacy `ScanStatusCard.svelte`). htmx polls it: every
/// second while a scan runs, every five seconds otherwise.
pub struct ScanCard {
    pub running: bool,
    pub has_progress: bool,
    pub stat_line: String,
    pub determinate: bool,
    pub percent: u32,
    pub counts: Option<Counts>,
    pub last_error: Option<String>,
    pub skipped_line: String,
}

impl ScanCard {
    pub fn poll_interval(&self) -> &'static str {
        if self.running { "1s" } else { "5s" }
    }
}

impl From<&ScanState> for ScanCard {
    fn from(s: &ScanState) -> Self {
        let progress = s.progress.as_ref();
        let total = progress.and_then(|p| p.total).filter(|t| *t > 0);
        let processed = progress.map_or(0, |p| p.processed);
        let percent = total.map_or(0, |t| {
            ((f64::from(processed) / f64::from(t)) * 100.0)
                .round()
                .min(100.0) as u32
        });
        let phase = match progress.map(|p| p.phase) {
            Some(Phase::Storing) => "Wird gespeichert",
            _ => "Ordner werden gelesen",
        };
        let root_label = progress
            .and_then(|p| p.root.as_deref())
            .and_then(|root| root.trim_end_matches('/').rsplit('/').next())
            .filter(|name| !name.is_empty());
        let stat_line = [
            Some(phase.to_owned()),
            root_label.map(str::to_owned),
            total.map(|t| format!("{processed} von {t}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let counts = match (s.running, progress, s.last_report) {
            (true, Some(p), _) => Some(Counts {
                new: p.new,
                updated: p.updated,
                unchanged: p.unchanged,
                removed: 0,
                skipped: p.skipped,
            }),
            (_, _, Some(r)) => Some(Counts {
                new: r.new,
                updated: r.updated,
                unchanged: r.unchanged,
                removed: r.removed,
                skipped: r.skipped,
            }),
            _ => None,
        };
        let skipped_line = if s.last_skipped.is_empty() {
            "—".to_owned()
        } else {
            s.last_skipped.join(", ")
        };
        ScanCard {
            running: s.running,
            has_progress: progress.is_some(),
            stat_line,
            determinate: total.is_some(),
            percent,
            counts,
            last_error: s.last_error.clone(),
            skipped_line,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::state::{ScanProgress, ScanReport};

    #[test]
    fn card_shows_phase_root_and_count_while_running() {
        let state = ScanState {
            running: true,
            progress: Some(ScanProgress {
                phase: Phase::Storing,
                root: Some("/media/Hörbücher/".into()),
                total: Some(8),
                processed: 2,
                new: 1,
                updated: 0,
                unchanged: 1,
                skipped: 0,
            }),
            ..Default::default()
        };
        let card = ScanCard::from(&state);
        assert_eq!(card.stat_line, "Wird gespeichert · Hörbücher · 2 von 8");
        assert_eq!(card.percent, 25);
        assert!(card.determinate);
        assert_eq!(card.counts.as_ref().unwrap().new, 1);
        assert_eq!(card.poll_interval(), "1s");
    }

    #[test]
    fn card_falls_back_to_the_last_report_when_idle() {
        let state = ScanState {
            last_report: Some(ScanReport {
                new: 3,
                removed: 1,
                ..Default::default()
            }),
            last_skipped: vec!["/a: kaputt".into()],
            ..Default::default()
        };
        let card = ScanCard::from(&state);
        let counts = card.counts.as_ref().unwrap();
        assert_eq!((counts.new, counts.removed), (3, 1));
        assert_eq!(card.skipped_line, "/a: kaputt");
        assert_eq!(card.poll_interval(), "5s");
        assert!(ScanCard::from(&ScanState::default()).counts.is_none());
    }
}
