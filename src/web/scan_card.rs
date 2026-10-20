//! What `partials/scan_card.html` shows (legacy `ScanStatusCard.svelte`, redesigned): while a scan
//! runs, which pass and phase it is in, how far that phase is, how long it has taken and probably
//! still takes; when idle, how the last scan ended. htmx polls the card: every second while a scan
//! runs, every five seconds otherwise.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};

use super::format::relative_day;
use crate::scanner::state::{Library, Phase, ScanProgress, ScanState};

/// A rate and a remaining time are only shown once the phase has run this long …
const ESTIMATE_AFTER: Duration = Duration::from_secs(3);
/// … and this many folders are through.
const ESTIMATE_MIN_DONE: u32 = 5;

pub struct ScanCard {
    pub running: bool,
    pub cancel_requested: bool,
    /// The heading: the library being scanned, or "Letzter Scan".
    pub title: String,
    pub pass_label: Option<String>,
    pub passes: Vec<Step>,
    pub phases: Vec<Step>,
    pub live: Option<Live>,
    pub last: Option<Last>,
    pub counts: Option<Counts>,
    /// The counts are those of a scan still running.
    pub counts_running: bool,
    pub last_error: Option<String>,
    pub log: Vec<String>,
}

/// One entry of the pass or phase list.
pub struct Step {
    pub label: &'static str,
    /// `waiting`, `running`, `done` or, for a pass whose root was unreadable, `skipped` (CSS hooks
    /// on `data-state`).
    pub state: &'static str,
    pub status: &'static str,
}

impl Step {
    pub fn active(&self) -> bool {
        self.state == "running"
    }
}

/// The running phase.
pub struct Live {
    pub phase: &'static str,
    /// The big number: "128" (of `total`) or, while listing, the folders found.
    pub amount: String,
    /// What follows it: "von 640 Ordnern", "Ordner gefunden".
    pub amount_label: String,
    /// `None` while the total is unknown: the bar has no value then.
    pub percent: Option<u32>,
    pub progress_text: String,
    pub elapsed: String,
    pub remaining: String,
    pub rate: String,
    pub current: Option<String>,
    pub note: Option<&'static str>,
}

/// How the last scan ended.
pub struct Last {
    pub outcome: &'static str,
    pub started: String,
    pub duration: Option<String>,
}

/// Formatted for display ("1.517").
pub struct Counts {
    pub new: String,
    pub updated: String,
    pub unchanged: String,
    pub skipped: String,
    /// Only for passes that finished: a running pass has not removed anything yet.
    pub removed: Option<String>,
}

impl ScanCard {
    pub fn poll_interval(&self) -> &'static str {
        if self.running { "1s" } else { "5s" }
    }

    pub fn new(s: &ScanState, now: DateTime<Utc>, instant: Instant) -> Self {
        let started = parse(s.started_at.as_deref());
        let finished = parse(s.finished_at.as_deref());
        let progress = s.progress.as_ref().filter(|_| s.running);
        let mut log: Vec<String> = Vec::new();
        if let Some(error) = &s.last_error {
            log.push(format!("Fehler: {error}"));
        }
        log.extend(s.last_skipped.iter().cloned());

        let counts = match (progress, s.last_report) {
            (Some(p), report) => Some(Counts {
                new: thousands(p.new),
                updated: thousands(p.updated),
                unchanged: thousands(p.unchanged),
                skipped: thousands(p.skipped),
                removed: report.map(|r| thousands(r.removed)),
            }),
            (None, Some(r)) => Some(Counts {
                new: thousands(r.new),
                updated: thousands(r.updated),
                unchanged: thousands(r.unchanged),
                skipped: thousands(r.skipped),
                removed: Some(thousands(r.removed)),
            }),
            (None, None) => None,
        };

        let (title, pass_label, passes, phases, live) = match progress {
            Some(p) => (
                library_label(p.library).to_owned(),
                Some(format!("Durchgang {} von {}", p.pass, p.passes)),
                pass_steps(p),
                phase_steps(p),
                Some(live(p, started, now, instant)),
            ),
            None if s.running => (
                "Scan wird vorbereitet".to_owned(),
                None,
                Vec::new(),
                Vec::new(),
                None,
            ),
            None => (
                if finished.is_some() {
                    "Letzter Scan"
                } else {
                    "Bibliotheken einlesen"
                }
                .to_owned(),
                None,
                Vec::new(),
                Vec::new(),
                None,
            ),
        };

        let last = match (s.running, finished) {
            (false, Some(finished)) => Some(Last {
                outcome: if s.cancelled {
                    "Abgebrochen · Teilergebnis"
                } else if s.last_error.is_some() {
                    "Mit Fehlern beendet"
                } else if !s.last_skipped.is_empty() {
                    "Abgeschlossen, Ordner übersprungen"
                } else {
                    "Abgeschlossen"
                },
                started: moment(started.unwrap_or(finished), now.with_timezone(&Local)),
                duration: started
                    .filter(|started| *started <= finished)
                    .map(|started| span((finished - started).to_std().unwrap_or_default())),
            }),
            _ => None,
        };

        ScanCard {
            running: s.running,
            cancel_requested: s.cancel_requested,
            title,
            pass_label,
            passes,
            phases,
            live,
            last,
            counts,
            counts_running: progress.is_some(),
            last_error: s.last_error.clone(),
            log,
        }
    }
}

impl From<&ScanState> for ScanCard {
    fn from(s: &ScanState) -> Self {
        ScanCard::new(s, Utc::now(), Instant::now())
    }
}

fn parse(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|v| v.with_timezone(&Utc))
}

fn library_label(library: Library) -> &'static str {
    match library {
        Library::Books => "Hörbücher",
        Library::Music => "Musik",
    }
}

/// The run's passes in their order: books first, then music (`scanner::run`).
fn pass_steps(p: &ScanProgress) -> Vec<Step> {
    let libraries: &[Library] = if p.passes == 2 {
        &[Library::Books, Library::Music]
    } else {
        std::slice::from_ref(&p.library)
    };
    libraries
        .iter()
        .zip(1..)
        .map(|(library, pass)| {
            let (state, status) = match pass.cmp(&p.pass) {
                std::cmp::Ordering::Less if p.unreadable.contains(&pass) => {
                    ("skipped", "Nicht lesbar")
                }
                std::cmp::Ordering::Less => ("done", "Fertig"),
                std::cmp::Ordering::Equal => ("running", "Läuft"),
                std::cmp::Ordering::Greater => ("waiting", "Wartet"),
            };
            Step {
                label: library_label(*library),
                state,
                status,
            }
        })
        .collect()
}

/// Listing (scanning without a total), reading (scanning with one), storing.
fn phase_steps(p: &ScanProgress) -> Vec<Step> {
    let current = match (p.phase, p.total) {
        (Phase::Scanning, None) => 0,
        (Phase::Scanning, Some(_)) => 1,
        (Phase::Storing, _) => 2,
    };
    ["Ordner suchen", "Ordner lesen", "Speichern"]
        .into_iter()
        .enumerate()
        .map(|(i, label)| {
            let (state, status) = match i.cmp(&current) {
                std::cmp::Ordering::Less => ("done", "erledigt"),
                std::cmp::Ordering::Equal => ("running", "läuft"),
                std::cmp::Ordering::Greater => ("waiting", "wartet"),
            };
            Step {
                label,
                state,
                status,
            }
        })
        .collect()
}

fn live(
    p: &ScanProgress,
    started: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    instant: Instant,
) -> Live {
    let library = library_label(p.library);
    let elapsed = started
        .and_then(|started| (now - started).to_std().ok())
        .map_or_else(|| "—".to_owned(), clock);
    let phase_time = instant.saturating_duration_since(p.phase_started);
    let estimable = phase_time >= ESTIMATE_AFTER && p.processed >= ESTIMATE_MIN_DONE;
    let rate = estimable.then(|| f64::from(p.processed) / phase_time.as_secs_f64());

    let Some(total) = p.total else {
        let found = thousands(p.found);
        return Live {
            phase: "Ordner suchen",
            amount_label: "Ordner gefunden".into(),
            progress_text: format!("{library}, Ordner suchen: {found} Ordner gefunden"),
            amount: found,
            percent: None,
            elapsed,
            remaining: "noch unbekannt".into(),
            rate: "—".into(),
            current: None,
            note: Some("Gesamtzahl noch unbekannt"),
        };
    };
    let phase = match p.phase {
        Phase::Scanning => "Ordner lesen",
        Phase::Storing => "Speichern",
    };
    let processed = p.processed.min(total);
    // Rounded down: 639 of 640 is not done yet.
    let percent = (u64::from(processed) * 100)
        .checked_div(u64::from(total))
        .unwrap_or(100) as u32;
    let remaining = match rate {
        _ if processed >= total => "gleich fertig".to_owned(),
        Some(rate) if rate > 0.0 => {
            let seconds = f64::from(total - processed) / rate;
            match span(Duration::from_secs_f64(seconds)) {
                short if short.starts_with("unter") => short,
                long => format!("ca. {long}"),
            }
        }
        _ => "wird berechnet".to_owned(),
    };
    Live {
        phase,
        amount: thousands(processed),
        amount_label: format!("von {} Ordnern", thousands(total)),
        percent: Some(percent),
        progress_text: format!(
            "{library}, {phase}: {} von {} Ordnern, {percent} Prozent",
            thousands(processed),
            thousands(total)
        ),
        elapsed,
        remaining,
        rate: rate.map_or_else(|| "—".to_owned(), decimal),
        current: p.current.clone(),
        note: (total == 0).then_some("Keine Ordner vorhanden"),
    }
}

/// "1.329", German grouping.
fn thousands(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// "4,2", one decimal with a comma.
fn decimal(value: f64) -> String {
    format!("{value:.1}").replace('.', ",")
}

/// "02:14", "1:02:14" from an hour on: a running clock.
fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// "unter 1 min", "3 min", "1 Std 5 min": a length in words.
fn span(d: Duration) -> String {
    let minutes = (d.as_secs_f64() / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, 0) if d.as_secs() < 30 => "unter 1 min".to_owned(),
        (0, 0) => "1 min".to_owned(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} Std"),
        (h, m) => format!("{h} Std {m} min"),
    }
}

/// "Heute, 07:42 Uhr", "Gestern, …", "5. Jan. 2026, …" in the server's local time.
fn moment(value: DateTime<Utc>, now: DateTime<Local>) -> String {
    format!(
        "{}, {} Uhr",
        relative_day(Some(value), now),
        value.with_timezone(&Local).format("%H:%M")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::state::ScanReport;

    fn progress(phase: Phase, total: Option<u32>, processed: u32) -> ScanProgress {
        ScanProgress {
            phase,
            root: Some("/media/music".into()),
            total,
            processed,
            new: 1,
            updated: 2,
            unchanged: 192,
            skipped: 0,
            library: Library::Music,
            pass: 2,
            passes: 2,
            found: 812,
            current: Some("Pink Floyd/The Wall".into()),
            unreadable: Vec::new(),
            phase_started: Instant::now(),
        }
    }

    fn running(p: ScanProgress) -> ScanState {
        ScanState {
            running: true,
            started_at: Some("2026-10-20T07:40:00.000Z".into()),
            last_report: Some(ScanReport {
                unchanged: 192,
                updated: 2,
                removed: 3,
                ..Default::default()
            }),
            progress: Some(p),
            ..Default::default()
        }
    }

    fn at(iso: &str) -> DateTime<Utc> {
        parse(Some(iso)).unwrap()
    }

    #[test]
    fn listing_counts_the_folders_found_without_a_total() {
        let p = progress(Phase::Scanning, None, 0);
        let card = ScanCard::new(&running(p), at("2026-10-20T07:42:14Z"), Instant::now());
        assert_eq!(card.title, "Musik");
        assert_eq!(card.pass_label.as_deref(), Some("Durchgang 2 von 2"));
        let states: Vec<_> = card.passes.iter().map(|s| (s.label, s.state)).collect();
        assert_eq!(states, [("Hörbücher", "done"), ("Musik", "running")]);
        let phases: Vec<_> = card.phases.iter().map(|s| s.state).collect();
        assert_eq!(phases, ["running", "waiting", "waiting"]);
        let live = card.live.as_ref().unwrap();
        assert_eq!((live.amount.as_str(), live.percent), ("812", None));
        assert_eq!(live.elapsed, "02:14");
        assert_eq!(card.poll_interval(), "1s");
    }

    #[test]
    fn a_pass_skipped_for_an_unreadable_root_is_not_shown_as_done() {
        let mut p = progress(Phase::Scanning, None, 0);
        p.unreadable = vec![1];
        let card = ScanCard::new(&running(p), Utc::now(), Instant::now());
        let books = &card.passes[0];
        assert_eq!((books.state, books.status), ("skipped", "Nicht lesbar"));
        assert_eq!(card.passes[1].state, "running");
    }

    #[test]
    fn reading_shows_count_percent_rate_and_remaining_time() {
        let mut p = progress(Phase::Scanning, Some(1329), 133);
        let now = Instant::now();
        p.phase_started = now - Duration::from_secs(10);
        let card = ScanCard::new(&running(p), at("2026-10-20T07:42:14Z"), now);
        let phases: Vec<_> = card.phases.iter().map(|s| s.state).collect();
        assert_eq!(phases, ["done", "running", "waiting"]);
        let live = card.live.as_ref().unwrap();
        assert_eq!(live.amount, "133");
        assert_eq!(live.amount_label, "von 1.329 Ordnern");
        assert_eq!(live.percent, Some(10));
        assert_eq!(live.rate, "13,3");
        // 1196 folders at 13.3/s: just under 90 s.
        assert_eq!(live.remaining, "ca. 1 min");
        assert_eq!(live.current.as_deref(), Some("Pink Floyd/The Wall"));
        // The removals of the finished books pass, the running pass's counts.
        let counts = card.counts.as_ref().unwrap();
        assert_eq!(counts.unchanged, "192");
        assert_eq!(counts.removed.as_deref(), Some("3"));
        assert!(card.counts_running);
    }

    #[test]
    fn no_estimate_before_the_phase_has_run_a_little() {
        let p = progress(Phase::Storing, Some(8), 2);
        let card = ScanCard::new(&running(p), Utc::now(), Instant::now());
        let live = card.live.as_ref().unwrap();
        assert_eq!(live.phase, "Speichern");
        assert_eq!(
            (live.rate.as_str(), live.remaining.as_str()),
            ("—", "wird berechnet")
        );
        assert_eq!(live.percent, Some(25));
    }

    #[test]
    fn an_almost_done_phase_is_not_rounded_up_and_an_empty_one_is_complete() {
        let p = progress(Phase::Scanning, Some(640), 639);
        let live = ScanCard::new(&running(p), Utc::now(), Instant::now())
            .live
            .unwrap();
        assert_eq!(live.percent, Some(99));
        let p = progress(Phase::Scanning, Some(0), 0);
        let live = ScanCard::new(&running(p), Utc::now(), Instant::now())
            .live
            .unwrap();
        assert_eq!(live.percent, Some(100));
        assert_eq!(live.note, Some("Keine Ordner vorhanden"));
    }

    #[test]
    fn idle_shows_how_the_last_scan_ended() {
        let state = ScanState {
            started_at: Some("2026-10-20T07:40:00.000Z".into()),
            finished_at: Some("2026-10-20T07:43:12.500Z".into()),
            last_report: Some(ScanReport {
                new: 3,
                removed: 1,
                ..Default::default()
            }),
            last_skipped: vec!["/a: kaputt".into()],
            ..Default::default()
        };
        let card = ScanCard::new(&state, at("2026-10-20T08:00:00Z"), Instant::now());
        assert_eq!(card.title, "Letzter Scan");
        let last = card.last.as_ref().unwrap();
        assert_eq!(last.outcome, "Abgeschlossen, Ordner übersprungen");
        assert_eq!(last.duration.as_deref(), Some("3 min"));
        assert!(last.started.ends_with(" Uhr"));
        let counts = card.counts.as_ref().unwrap();
        assert_eq!(counts.new, "3");
        assert_eq!(counts.removed.as_deref(), Some("1"));
        assert_eq!(card.log, ["/a: kaputt"]);
        assert_eq!(card.poll_interval(), "5s");

        let fresh = ScanCard::from(&ScanState::default());
        assert!(fresh.counts.is_none() && fresh.last.is_none());
        assert_eq!(fresh.title, "Bibliotheken einlesen");
    }

    #[test]
    fn a_cancelled_scan_says_its_result_is_partial() {
        let state = ScanState {
            finished_at: Some("2026-10-20T07:43:12.500Z".into()),
            cancelled: true,
            ..Default::default()
        };
        let card = ScanCard::from(&state);
        assert_eq!(card.last.unwrap().outcome, "Abgebrochen · Teilergebnis");
    }

    #[test]
    fn formats() {
        assert_eq!(thousands(1329), "1.329");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1.000.000");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(span(Duration::from_secs(10)), "unter 1 min");
        assert_eq!(span(Duration::from_secs(3900)), "1 Std 5 min");
    }
}
