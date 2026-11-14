//! What admins set under Einstellungen → Server and through `GET`/`PATCH /server/settings`:
//! intervals of the background jobs, the switches of optional features and the language model.
//!
//! One JSON document in `server_settings`, read at start and then held in memory ([`Shared`]):
//! every reader takes the current value, and a change wakes whoever waits on it ([`Ticker`]),
//! so nothing needs a restart. These used to be environment variables; the first start with this
//! table takes them over once ([`from_env`]) and from then on they are not read any more.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::ai::AiConfig;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const MAX_INTERVAL_HOURS: f64 = 168.0;
pub const MAX_ARTIST_INFO_DAYS: f64 = 365.0;
pub const MAX_LOUDNESS_THREADS: u32 = 16;

/// The variables taken over at the first start, and ignored after it.
const FORMER_VARIABLES: [&str; 13] = [
    "AUTOREVERSE_SCAN_HOURS",
    "AUTOREVERSE_PODCAST_REFRESH_HOURS",
    "AUTOREVERSE_ARTIST_INFO",
    "AUTOREVERSE_ARTIST_INFO_DAYS",
    "AUTOREVERSE_LOUDNESS",
    "AUTOREVERSE_LOUDNESS_THREADS",
    "AUTOREVERSE_RADIO",
    "AUTOREVERSE_RADIO_DIRECTORY",
    "AUTOREVERSE_DEVICES",
    "AUTOREVERSE_AI_PROVIDER",
    "AUTOREVERSE_AI_MODEL",
    "AUTOREVERSE_AI_URL",
    "AUTOREVERSE_AI_API_KEY",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// How often the library is scanned by itself; 0 = never.
    pub scan_hours: f64,
    /// How often every podcast feed is refreshed; 0 = never.
    pub podcast_refresh_hours: f64,
    /// Whether TheAudioDB and Apple's catalogue are asked about artists.
    pub artist_info: bool,
    /// After how many days an artist's answers are due again; 0 = never.
    pub artist_info_days: f64,
    /// Whether music without loudness tags is measured. Tags are read either way.
    pub loudness: bool,
    /// Albums decoded at once; 0 = half the cores, at most 4.
    pub loudness_threads: u32,
    /// Internet radio stations.
    pub radio: bool,
    /// Whether stations can be searched in radio-browser.info; counts only while `radio` does.
    pub radio_directory: bool,
    /// Remote control between the devices of an account.
    pub devices: bool,
    pub ai: AiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            scan_hours: 1.0,
            podcast_refresh_hours: 1.0,
            artist_info: true,
            artist_info_days: 30.0,
            loudness: true,
            loudness_threads: 0,
            radio: true,
            radio_directory: true,
            devices: true,
            ai: AiSettings::default(),
        }
    }
}

/// The language model. No provider: suggestions are off.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// `anthropic` or `ollama`.
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Another address for the provider's API (a proxy, Ollama on another machine).
    pub url: Option<String>,
    /// Never sent to a client once stored; they only learn whether there is one.
    pub api_key: Option<String>,
}

/// By hand, so that a logged setting never contains the key.
impl std::fmt::Debug for AiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiSettings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("url", &self.url)
            .field("api_key", &self.api_key.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Settings {
    /// Whether the directory may be asked: only with radio on.
    pub fn radio_directory_on(&self) -> bool {
        self.radio && self.radio_directory
    }

    pub fn loudness_threads(&self) -> usize {
        match self.loudness_threads {
            0 => crate::scanner::loudness::default_threads(),
            n => n as usize,
        }
    }

    /// The model to ask, or `None` when suggestions are off or not set up completely.
    pub fn ai(&self) -> Option<AiConfig> {
        AiConfig::from_settings(&self.ai).ok().flatten()
    }
}

/// The settings of this process: read by anybody, replaced by a change, watched by the jobs.
#[derive(Clone)]
pub struct Shared(std::sync::Arc<watch::Sender<Settings>>);

impl Default for Shared {
    fn default() -> Self {
        Shared(std::sync::Arc::new(watch::channel(Settings::default()).0))
    }
}

impl Shared {
    pub fn get(&self) -> Settings {
        self.0.borrow().clone()
    }

    pub fn set(&self, settings: Settings) {
        self.0.send_replace(settings);
    }

    /// Changes them in place (tests; the server goes through [`change`]).
    pub fn update(&self, change: impl FnOnce(&mut Settings)) {
        self.0.send_modify(change);
    }

    pub fn subscribe(&self) -> watch::Receiver<Settings> {
        self.0.subscribe()
    }
}

fn hours(raw: Option<String>, default: f64) -> f64 {
    let Some(raw) = raw else { return default };
    match raw.trim().parse::<f64>() {
        Ok(h) if h.is_finite() && h > 0.0 => h,
        _ => 0.0,
    }
}

/// What the former environment variables said, for the first start with the table.
pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Settings {
    let switched_on = |key: &str| {
        !matches!(
            lookup(key).as_deref().map(str::trim),
            Some("0" | "off" | "false" | "no")
        )
    };
    let text = |key: &str| {
        lookup(key)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let defaults = Settings::default();
    Settings {
        scan_hours: hours(lookup("AUTOREVERSE_SCAN_HOURS"), defaults.scan_hours),
        podcast_refresh_hours: hours(
            lookup("AUTOREVERSE_PODCAST_REFRESH_HOURS"),
            defaults.podcast_refresh_hours,
        ),
        artist_info: switched_on("AUTOREVERSE_ARTIST_INFO"),
        artist_info_days: hours(
            lookup("AUTOREVERSE_ARTIST_INFO_DAYS"),
            defaults.artist_info_days,
        )
        .min(MAX_ARTIST_INFO_DAYS),
        loudness: switched_on("AUTOREVERSE_LOUDNESS"),
        loudness_threads: lookup("AUTOREVERSE_LOUDNESS_THREADS")
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .map(|n| n.min(MAX_LOUDNESS_THREADS))
            .unwrap_or(0),
        radio: switched_on("AUTOREVERSE_RADIO"),
        radio_directory: switched_on("AUTOREVERSE_RADIO_DIRECTORY"),
        devices: switched_on("AUTOREVERSE_DEVICES"),
        ai: AiSettings {
            provider: text("AUTOREVERSE_AI_PROVIDER").map(|p| p.to_lowercase()),
            model: text("AUTOREVERSE_AI_MODEL"),
            url: text("AUTOREVERSE_AI_URL"),
            api_key: text("AUTOREVERSE_AI_API_KEY"),
        },
    }
}

/// The stored settings; at the first start the former environment variables, stored then.
pub async fn load(
    db: &PgPool,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Settings, sqlx::Error> {
    let still_set: Vec<&str> = FORMER_VARIABLES
        .into_iter()
        .filter(|key| lookup(key).is_some())
        .collect();
    let stored: Option<Value> = sqlx::query_scalar("SELECT value FROM server_settings")
        .fetch_optional(db)
        .await?;
    if let Some(value) = stored {
        if !still_set.is_empty() {
            tracing::warn!(
                "[einstellungen] {} werden nicht mehr gelesen; die Werte stehen unter Einstellungen → Server",
                still_set.join(", ")
            );
        }
        return Ok(serde_json::from_value(value).unwrap_or_else(|e| {
            tracing::error!(
                "[einstellungen] gespeicherte Einstellungen unlesbar, Standardwerte: {e}"
            );
            Settings::default()
        }));
    }
    let settings = from_env(&lookup);
    save(db, &settings).await?;
    if !still_set.is_empty() {
        tracing::info!(
            "[einstellungen] {} übernommen; ab jetzt unter Einstellungen → Server",
            still_set.join(", ")
        );
    }
    Ok(settings)
}

async fn save(db: &PgPool, settings: &Settings) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO server_settings (id, value) VALUES (true, $1)
         ON CONFLICT (id) DO UPDATE SET value = excluded.value, updated_at = now()",
    )
    .bind(sqlx::types::Json(settings))
    .execute(db)
    .await?;
    Ok(())
}

/// The settings as clients see them: everything but the key, of which only whether there is one.
#[derive(Serialize, Debug, PartialEq)]
pub struct Out {
    pub scan_hours: f64,
    pub podcast_refresh_hours: f64,
    pub artist_info: bool,
    pub artist_info_days: f64,
    pub loudness: bool,
    pub loudness_threads: u32,
    /// What `loudness_threads` 0 comes to on this machine.
    pub loudness_threads_default: usize,
    pub radio: bool,
    pub radio_directory: bool,
    pub devices: bool,
    pub ai: AiOut,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct AiOut {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub url: Option<String>,
    pub has_api_key: bool,
    /// Whether suggestions work with this: a provider and all it needs.
    pub enabled: bool,
    /// The providers there are, and the models a client may offer for Anthropic.
    pub providers: [&'static str; 2],
    pub anthropic_models: &'static [&'static str],
}

impl From<&Settings> for Out {
    fn from(s: &Settings) -> Self {
        Out {
            scan_hours: s.scan_hours,
            podcast_refresh_hours: s.podcast_refresh_hours,
            artist_info: s.artist_info,
            artist_info_days: s.artist_info_days,
            loudness: s.loudness,
            loudness_threads: s.loudness_threads,
            loudness_threads_default: crate::scanner::loudness::default_threads(),
            radio: s.radio,
            radio_directory: s.radio_directory,
            devices: s.devices,
            ai: AiOut {
                provider: s.ai.provider.clone(),
                model: s.ai.model.clone(),
                url: s.ai.url.clone(),
                has_api_key: s.ai.api_key.is_some(),
                enabled: s.ai().is_some(),
                providers: ["anthropic", "ollama"],
                anthropic_models: crate::ai::ANTHROPIC_MODELS,
            },
        }
    }
}

fn number(body: &Map<String, Value>, key: &str, max: f64, invalid: &str) -> ApiResult<Option<f64>> {
    match body.get(key) {
        None => Ok(None),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) if (0.0..=max).contains(&v) => Ok(Some(v)),
            _ => Err(ApiError::unprocessable(invalid)),
        },
        Some(_) => Err(ApiError::unprocessable(invalid)),
    }
}

fn switch(body: &Map<String, Value>, key: &str) -> ApiResult<Option<bool>> {
    match body.get(key) {
        None => Ok(None),
        Some(Value::Bool(v)) => Ok(Some(*v)),
        Some(_) => Err(ApiError::unprocessable(format!(
            "„{key}“ muss wahr oder falsch sein"
        ))),
    }
}

/// An optional text: `null` or blank clears it. Outer `None`: not in the body.
fn text(body: &Map<String, Value>, key: &str) -> ApiResult<Option<Option<String>>> {
    match body.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(v)) => {
            let v = v.trim();
            Ok(Some((!v.is_empty()).then(|| v.to_owned())))
        }
        Some(_) => Err(ApiError::unprocessable(format!(
            "„{key}“ muss ein Text oder null sein"
        ))),
    }
}

/// `current` with what `body` changes: every field optional, unknown ones ignored, at least one
/// known one required. The result is checked as a whole (a provider needs its key or model).
pub fn apply(current: &Settings, body: &Map<String, Value>) -> ApiResult<Settings> {
    let mut next = current.clone();
    let mut known = false;
    let hours_text = format!("muss zwischen 0 und {MAX_INTERVAL_HOURS} Stunden liegen");
    if let Some(v) = number(
        body,
        "scan_hours",
        MAX_INTERVAL_HOURS,
        &format!("Das Scan-Intervall {hours_text}"),
    )? {
        next.scan_hours = v;
        known = true;
    }
    if let Some(v) = number(
        body,
        "podcast_refresh_hours",
        MAX_INTERVAL_HOURS,
        &format!("Das Podcast-Intervall {hours_text}"),
    )? {
        next.podcast_refresh_hours = v;
        known = true;
    }
    if let Some(v) = number(
        body,
        "artist_info_days",
        MAX_ARTIST_INFO_DAYS,
        &format!(
            "Das Intervall der Interpreten-Angaben muss zwischen 0 und {MAX_ARTIST_INFO_DAYS} Tagen liegen"
        ),
    )? {
        next.artist_info_days = v;
        known = true;
    }
    if let Some(v) = number(
        body,
        "loudness_threads",
        MAX_LOUDNESS_THREADS as f64,
        &format!(
            "Die Zahl der Mess-Threads muss eine ganze Zahl zwischen 0 und {MAX_LOUDNESS_THREADS} sein"
        ),
    )? {
        if v.fract() != 0.0 {
            return Err(ApiError::unprocessable(format!(
                "Die Zahl der Mess-Threads muss eine ganze Zahl zwischen 0 und {MAX_LOUDNESS_THREADS} sein"
            )));
        }
        next.loudness_threads = v as u32;
        known = true;
    }
    for (key, field) in [
        ("artist_info", &mut next.artist_info),
        ("loudness", &mut next.loudness),
        ("radio", &mut next.radio),
        ("radio_directory", &mut next.radio_directory),
        ("devices", &mut next.devices),
    ] {
        if let Some(v) = switch(body, key)? {
            *field = v;
            known = true;
        }
    }
    match body.get("ai") {
        None => {}
        Some(Value::Object(ai)) => {
            known = true;
            if let Some(v) = text(ai, "provider")? {
                next.ai.provider = v.map(|p| p.to_lowercase());
            }
            if let Some(v) = text(ai, "model")? {
                next.ai.model = v;
            }
            if let Some(v) = text(ai, "url")? {
                if let Some(url) = &v {
                    let parsed = reqwest::Url::parse(url).ok();
                    if !parsed.is_some_and(|u| matches!(u.scheme(), "http" | "https")) {
                        return Err(ApiError::unprocessable(
                            "Die Adresse des KI-Anbieters muss mit http:// oder https:// beginnen",
                        ));
                    }
                }
                next.ai.url = v;
            }
            if let Some(v) = text(ai, "api_key")? {
                next.ai.api_key = v;
            }
        }
        Some(_) => {
            return Err(ApiError::unprocessable("„ai“ muss ein Objekt sein"));
        }
    }
    if !known {
        return Err(ApiError::unprocessable(
            "Keine bekannte Einstellung angegeben",
        ));
    }
    AiConfig::from_settings(&next.ai).map_err(ApiError::unprocessable)?;
    Ok(next)
}

/// Applies a change, stores it and puts it into effect: the jobs wake by themselves, a measuring
/// pass that was switched on starts now.
pub async fn change(state: &AppState, body: &Map<String, Value>) -> ApiResult<Settings> {
    let before = state.settings.get();
    let next = apply(&before, body)?;
    save(&state.db, &next).await?;
    state.settings.set(next.clone());
    if next.loudness && !before.loudness {
        crate::scanner::loudness::start(state);
    }
    Ok(next)
}

/// Waits for the next run of a job whose period is a setting. The first run comes one period
/// after the start or after the period changed; a period of `None` (switched off) waits until
/// it is switched on. Other settings changing does not move the next run.
pub struct Ticker {
    changes: watch::Receiver<Settings>,
    period_of: fn(&Settings) -> Option<Duration>,
    period: Option<Duration>,
    next: Option<Instant>,
    /// Told the new period whenever it changes, the first time included (for the log).
    on_change: fn(Option<Duration>),
}

impl Ticker {
    pub fn new(
        shared: &Shared,
        period_of: fn(&Settings) -> Option<Duration>,
        on_change: fn(Option<Duration>),
    ) -> Self {
        let changes = shared.subscribe();
        let period = period_of(&changes.borrow());
        on_change(period);
        Ticker {
            changes,
            period_of,
            period,
            next: period.map(|p| Instant::now() + p),
            on_change,
        }
    }

    /// Returns when the job is due; `false` when the settings are gone (the server stops).
    pub async fn tick(&mut self) -> bool {
        loop {
            let changed = match self.next {
                Some(at) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(at) => {
                            self.next = self.period.map(|p| Instant::now() + p);
                            return true;
                        }
                        changed = self.changes.changed() => changed,
                    }
                }
                None => self.changes.changed().await,
            };
            if changed.is_err() {
                return false;
            }
            let period = (self.period_of)(&self.changes.borrow_and_update());
            if period != self.period {
                self.period = period;
                self.next = period.map(|p| Instant::now() + p);
                (self.on_change)(period);
            }
        }
    }
}

/// `hours` as a period; 0 or less is off.
pub fn period_hours(hours: f64) -> Option<Duration> {
    (hours > 0.0).then(|| Duration::from_secs_f64(hours * 3600.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn body(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn the_former_variables_come_over_as_they_were_meant() {
        let env: HashMap<&str, &str> = [
            ("AUTOREVERSE_SCAN_HOURS", "0"),
            ("AUTOREVERSE_PODCAST_REFRESH_HOURS", "12"),
            ("AUTOREVERSE_ARTIST_INFO_DAYS", "7"),
            ("AUTOREVERSE_RADIO", "off"),
            ("AUTOREVERSE_LOUDNESS_THREADS", "99"),
            ("AUTOREVERSE_AI_PROVIDER", " Anthropic "),
            ("AUTOREVERSE_AI_MODEL", "claude-sonnet-5-5"),
            ("AUTOREVERSE_AI_API_KEY", "geheim"),
        ]
        .into();
        let s = from_env(|k| env.get(k).map(|v| v.to_string()));
        assert_eq!(s.scan_hours, 0.0);
        assert_eq!(s.podcast_refresh_hours, 12.0);
        assert_eq!(s.artist_info_days, 7.0);
        assert!(!s.radio && s.radio_directory && !s.radio_directory_on());
        assert!(s.devices && s.loudness && s.artist_info);
        assert_eq!(s.loudness_threads, MAX_LOUDNESS_THREADS);
        assert_eq!(s.ai.provider.as_deref(), Some("anthropic"));
        assert!(s.ai().is_some());
        assert_eq!(from_env(|_| None), Settings::default());
    }

    #[test]
    fn a_change_names_only_what_changes_and_is_checked() {
        let s = Settings::default();
        let next = apply(
            &s,
            &body(json!({"scan_hours": 6, "radio": false, "other": 1})),
        )
        .unwrap();
        assert_eq!(next.scan_hours, 6.0);
        assert!(!next.radio);
        assert_eq!(next.podcast_refresh_hours, 1.0);

        let fails = |v: Value| apply(&s, &body(v)).unwrap_err().detail;
        assert_eq!(fails(json!({})), "Keine bekannte Einstellung angegeben");
        assert_eq!(
            fails(json!({"other": 1})),
            "Keine bekannte Einstellung angegeben"
        );
        assert_eq!(
            fails(json!({"scan_hours": 169})),
            "Das Scan-Intervall muss zwischen 0 und 168 Stunden liegen"
        );
        assert_eq!(
            fails(json!({"scan_hours": "6"})),
            "Das Scan-Intervall muss zwischen 0 und 168 Stunden liegen"
        );
        assert_eq!(
            fails(json!({"radio": 1})),
            "„radio“ muss wahr oder falsch sein"
        );
        assert_eq!(
            fails(json!({"loudness_threads": 1.5})),
            "Die Zahl der Mess-Threads muss eine ganze Zahl zwischen 0 und 16 sein"
        );
        assert_eq!(
            fails(json!({"ai": {"provider": "anthropic"}})),
            "Für Anthropic fehlt der API-Schlüssel"
        );
        assert_eq!(
            fails(json!({"ai": {"provider": "ollama"}})),
            "Für Ollama fehlt das Modell"
        );
        assert_eq!(
            fails(json!({"ai": {"provider": "openai"}})),
            "Unbekannter KI-Anbieter „openai“"
        );
        assert_eq!(
            fails(json!({"ai": {"url": "ftp://x"}})),
            "Die Adresse des KI-Anbieters muss mit http:// oder https:// beginnen"
        );
    }

    #[test]
    fn the_key_is_kept_unless_named_and_never_shown() {
        let s = apply(
            &Settings::default(),
            &body(json!({"ai": {"provider": "anthropic", "api_key": "geheim"}})),
        )
        .unwrap();
        let renamed = apply(&s, &body(json!({"ai": {"model": "claude-opus-5-5"}}))).unwrap();
        assert_eq!(renamed.ai.api_key.as_deref(), Some("geheim"));
        let out = serde_json::to_string(&Out::from(&renamed)).unwrap();
        assert!(!out.contains("geheim"), "{out}");
        assert!(out.contains("\"has_api_key\":true"));
        assert!(!format!("{renamed:?}").contains("geheim"));
        // Clearing the key of the provider that needs it is refused; switching off is not.
        assert!(apply(&renamed, &body(json!({"ai": {"api_key": null}}))).is_err());
        let off = apply(
            &renamed,
            &body(json!({"ai": {"provider": null, "api_key": ""}})),
        )
        .unwrap();
        assert_eq!(off.ai.api_key, None);
        assert!(off.ai().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn the_ticker_follows_the_setting_without_a_restart() {
        let shared = Shared::default();
        shared.update(|s| s.scan_hours = 0.0);
        let mut ticker = Ticker::new(&shared, |s| period_hours(s.scan_hours), |_| {});
        let start = Instant::now();
        let waiting = tokio::spawn(async move {
            ticker.tick().await;
            Instant::now()
        });
        tokio::time::sleep(Duration::from_secs(10 * 3600)).await;
        assert!(!waiting.is_finished(), "off means off");
        shared.update(|s| s.scan_hours = 2.0);
        // Another setting changing does not move the run.
        tokio::time::sleep(Duration::from_secs(3600)).await;
        shared.update(|s| s.radio = false);
        let ran = waiting.await.unwrap();
        assert_eq!(ran - start, Duration::from_secs(12 * 3600));
    }
}
