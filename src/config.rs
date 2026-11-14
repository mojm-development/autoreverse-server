//! Runtime configuration from the environment (legacy `config.ts`).
//!
//! Only process-level settings live here. The library roots are configured at runtime and stored
//! in `library_config`, not in the environment.

use crate::auth::client_address::TrustedProxies;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

pub const DEFAULT_DATABASE_URL: &str =
    "postgresql://autoreverse:autoreverse@localhost:5434/autoreverse";

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    /// Server-managed files: covers, downloaded episodes, artist images.
    pub data_dir: PathBuf,
    pub cover_dir: PathBuf,
    pub podcasts_dir: PathBuf,
    pub artists_dir: PathBuf,
    /// Uploaded playlist covers.
    pub playlists_dir: PathBuf,
    /// Scaled-down covers and artist images, rendered on first request; safe to delete.
    pub thumbs_dir: PathBuf,
    /// 0 turns the background refresh off.
    pub podcast_refresh_hours: f64,
    /// How often the library is scanned by itself; 0 turns that off.
    pub scan_hours: f64,
    /// The iTunes Search API; `AUTOREVERSE_PODCAST_DIRECTORY_URL` points it elsewhere (the e2e
    /// run serves a fixture, so no browser test reaches the internet).
    pub podcast_directory_url: String,
    /// Whether biographies, pictures and top tracks of artists are fetched from the catalogues
    /// below; `AUTOREVERSE_ARTIST_INFO=0` keeps the server from asking anybody.
    pub artist_info: bool,
    /// TheAudioDB (`AUTOREVERSE_AUDIODB_URL`) and the music catalogue
    /// (`AUTOREVERSE_MUSIC_CATALOG_URL`); tests point both at a local server.
    pub audiodb_url: String,
    pub music_catalog_url: String,
    /// Internet radio stations (`stations`); `AUTOREVERSE_RADIO=0` takes the whole feature away.
    pub radio: bool,
    /// Whether stations can be searched in the public directory; `AUTOREVERSE_RADIO_DIRECTORY=0`
    /// keeps the server from asking it, stations are then added by their address only. Always
    /// false while `radio` is.
    pub radio_directory: bool,
    /// radio-browser.info, or wherever `AUTOREVERSE_RADIO_DIRECTORY_URL` points (tests: a local
    /// server).
    pub radio_directory_url: String,
    /// Station logos.
    pub radio_dir: PathBuf,
    /// Remote control between the devices of an account (`devices`); `AUTOREVERSE_DEVICES=0`
    /// takes the routes away.
    pub devices: bool,
    /// Whether music without loudness tags is measured in the background
    /// (`AUTOREVERSE_LOUDNESS=0`: no). Tags are read either way.
    pub loudness: bool,
    /// Albums decoded at once (`AUTOREVERSE_LOUDNESS_THREADS`); at least one.
    pub loudness_threads: usize,
    /// First-admin bootstrap, only used against an empty `users` table.
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub host: IpAddr,
    pub port: u16,
    /// Public origin as the browser sees it (`https://audio.example.com`). Needed behind a
    /// TLS-terminating proxy: it decides the cookie's `Secure` flag and what the cross-site form
    /// check accepts as the server's own origin.
    pub origin: Option<String>,
    /// Proxies whose `X-Forwarded-For` is believed (`AUTOREVERSE_TRUSTED_PROXIES`).
    pub trusted_proxies: TrustedProxies,
    /// Language-model suggestions; `None` leaves them off.
    pub ai: Option<crate::ai::AiConfig>,
    /// CSS, JS, icons. `AUTOREVERSE_STATIC`, else `static` (relative); the server resolves it
    /// with [`find_static_dir`] at start.
    pub static_dir: PathBuf,
}

const DEFAULT_SCAN_HOURS: f64 = 1.0;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("PORT ist keine gültige Portnummer: {0}")]
    Port(String),
    #[error("HOST ist keine gültige IP-Adresse: {0}")]
    Host(String),
    #[error("AUTOREVERSE_TRUSTED_PROXIES enthält einen ungültigen Eintrag: {0}")]
    TrustedProxies(String),
    #[error("KI-Einstellungen: {0}")]
    Ai(String),
    #[error(
        "Statische Dateien nicht gefunden (gesucht: {0}). AUTOREVERSE_STATIC auf das Verzeichnis \
         `static` des Servers setzen."
    )]
    StaticMissing(String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Reads every value through `lookup`, so tests can pass a map instead of the process env.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let data_dir = PathBuf::from(lookup("AUTOREVERSE_DATA").unwrap_or_else(|| "./data".into()));
        // An empty value counts as unset, as `!name` did in legacy bootstrap.ts.
        let non_empty = |key: &str| lookup(key).filter(|v| !v.is_empty());
        let port = match lookup("PORT") {
            None => 3000,
            Some(raw) => raw.trim().parse().map_err(|_| ConfigError::Port(raw))?,
        };
        let host = match lookup("HOST") {
            None => IpAddr::from([0, 0, 0, 0]),
            Some(raw) => raw.trim().parse().map_err(|_| ConfigError::Host(raw))?,
        };
        let switched_on = |key: &str| {
            !matches!(
                lookup(key).as_deref().map(str::trim),
                Some("0" | "off" | "false" | "no")
            )
        };
        let radio = switched_on("AUTOREVERSE_RADIO");
        Ok(Config {
            database_url: lookup("DATABASE_URL").unwrap_or_else(|| DEFAULT_DATABASE_URL.into()),
            cover_dir: data_dir.join("covers"),
            podcasts_dir: data_dir.join("podcasts"),
            artists_dir: data_dir.join("artists"),
            playlists_dir: data_dir.join("playlists"),
            thumbs_dir: data_dir.join("thumbs"),
            radio_dir: data_dir.join("radio"),
            devices: switched_on("AUTOREVERSE_DEVICES"),
            loudness: switched_on("AUTOREVERSE_LOUDNESS"),
            loudness_threads: lookup("AUTOREVERSE_LOUDNESS_THREADS")
                .and_then(|raw| raw.trim().parse::<usize>().ok())
                .filter(|threads| *threads > 0)
                // More than this only fights the players for the disk.
                .map(|threads| threads.min(64))
                .unwrap_or_else(crate::scanner::loudness::default_threads),
            data_dir,
            podcast_refresh_hours: hours_setting(lookup("AUTOREVERSE_PODCAST_REFRESH_HOURS"), 1.0),
            scan_hours: hours_setting(lookup("AUTOREVERSE_SCAN_HOURS"), DEFAULT_SCAN_HOURS),
            podcast_directory_url: non_empty("AUTOREVERSE_PODCAST_DIRECTORY_URL")
                .unwrap_or_else(|| crate::podcasts::directory::DIRECTORY_URL.into()),
            artist_info: switched_on("AUTOREVERSE_ARTIST_INFO"),
            radio,
            radio_directory: radio && switched_on("AUTOREVERSE_RADIO_DIRECTORY"),
            radio_directory_url: non_empty("AUTOREVERSE_RADIO_DIRECTORY_URL")
                .map(|url| url.trim().trim_end_matches('/').to_owned())
                .unwrap_or_else(|| crate::stations::directory::DIRECTORY_URL.into()),

            audiodb_url: non_empty("AUTOREVERSE_AUDIODB_URL")
                .unwrap_or_else(|| crate::library::artist_info::AUDIODB_URL.into()),
            music_catalog_url: non_empty("AUTOREVERSE_MUSIC_CATALOG_URL")
                .unwrap_or_else(|| crate::library::artist_info::CATALOG_URL.into()),
            admin_user: non_empty("AUTOREVERSE_ADMIN_USER"),
            admin_password: non_empty("AUTOREVERSE_ADMIN_PASSWORD"),
            host,
            port,
            origin: non_empty("ORIGIN").map(|o| o.trim_end_matches('/').to_owned()),
            trusted_proxies: TrustedProxies::parse(
                &lookup("AUTOREVERSE_TRUSTED_PROXIES").unwrap_or_default(),
            )
            .map_err(ConfigError::TrustedProxies)?,
            ai: crate::ai::AiConfig::from_lookup(&lookup).map_err(ConfigError::Ai)?,
            static_dir: non_empty("AUTOREVERSE_STATIC")
                .map_or_else(|| PathBuf::from("static"), PathBuf::from),
        })
    }

    /// Whether the browser reaches us over TLS — only known when ORIGIN says so.
    pub fn secure_cookies(&self) -> bool {
        self.origin
            .as_deref()
            .is_some_and(|o| o.starts_with("https://"))
    }
}

/// A file every static directory has; its absence means the wrong directory.
const STATIC_MARKER: &str = "css/app.css";

/// Where the CSS and JS are: `explicit` (`AUTOREVERSE_STATIC`) if given, else `static` beside the
/// binary (an installed or container layout), else `static` in the working directory (`cargo run`).
/// Started from elsewhere, the server used to serve pages without CSS and JS and say nothing; now
/// it refuses to start.
pub fn find_static_dir(
    explicit: Option<&Path>,
    exe: Option<&Path>,
) -> Result<PathBuf, ConfigError> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(dir) => vec![dir.to_path_buf()],
        None => exe
            .and_then(Path::parent)
            .map(|dir| dir.join("static"))
            .into_iter()
            .chain([PathBuf::from("static")])
            .collect(),
    };
    candidates
        .iter()
        .find(|dir| dir.join(STATIC_MARKER).is_file())
        .cloned()
        .ok_or_else(|| {
            ConfigError::StaticMissing(
                candidates
                    .iter()
                    .map(|dir| dir.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
}

/// `Math.max(0, Number(raw ?? 1) || 0)`: unset means hourly, anything that is not a positive
/// number turns the refresh off.
fn hours_setting(raw: Option<String>, default: f64) -> f64 {
    let Some(raw) = raw else { return default };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    match trimmed.parse::<f64>() {
        Ok(h) if h.is_finite() && h > 0.0 => h,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(pairs: &[(&str, &str)]) -> Config {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned()).unwrap()
    }

    #[test]
    fn applies_defaults_for_data_dir_and_database_url() {
        let config = load(&[]);
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.cover_dir, PathBuf::from("./data/covers"));
        assert_eq!(config.podcasts_dir, PathBuf::from("./data/podcasts"));
        assert_eq!(config.database_url, DEFAULT_DATABASE_URL);
        assert_eq!(config.port, 3000);
        assert_eq!(config.host.to_string(), "0.0.0.0");
    }

    #[test]
    fn honors_explicit_data_dir_and_database_url() {
        let config = load(&[
            ("AUTOREVERSE_DATA", "/data"),
            ("DATABASE_URL", "postgresql://x/y"),
        ]);
        assert_eq!(config.cover_dir, PathBuf::from("/data/covers"));
        assert_eq!(config.database_url, "postgresql://x/y");
    }

    #[test]
    fn podcast_refresh_defaults_to_every_hour() {
        assert_eq!(load(&[]).podcast_refresh_hours, 1.0);
    }

    #[test]
    fn podcast_refresh_is_off_for_zero_and_non_numbers() {
        for raw in ["0", "nope", "-3", ""] {
            let config = load(&[("AUTOREVERSE_PODCAST_REFRESH_HOURS", raw)]);
            assert_eq!(config.podcast_refresh_hours, 0.0, "{raw:?}");
        }
    }

    #[test]
    fn podcast_refresh_takes_a_custom_interval() {
        let config = load(&[("AUTOREVERSE_PODCAST_REFRESH_HOURS", "12")]);
        assert_eq!(config.podcast_refresh_hours, 12.0);
        assert_eq!(load(&[]).scan_hours, DEFAULT_SCAN_HOURS);
        assert_eq!(load(&[("AUTOREVERSE_SCAN_HOURS", "0")]).scan_hours, 0.0);
        assert_eq!(load(&[("AUTOREVERSE_SCAN_HOURS", "2")]).scan_hours, 2.0);
    }

    #[test]
    fn artist_images_sit_beside_the_other_server_managed_files() {
        let config = load(&[("AUTOREVERSE_DATA", "/data")]);
        assert_eq!(config.artists_dir, PathBuf::from("/data/artists"));
    }

    #[test]
    fn remote_control_is_on_unless_switched_off() {
        assert!(load(&[]).devices);
        assert!(!load(&[("AUTOREVERSE_DEVICES", "0")]).devices);
        assert!(load(&[("AUTOREVERSE_DEVICES", "1")]).devices);
    }

    #[test]
    fn radio_and_its_directory_are_on_unless_switched_off() {
        let config = load(&[("AUTOREVERSE_DATA", "/data")]);
        assert!(config.radio && config.radio_directory);
        assert_eq!(
            config.radio_directory_url,
            "https://all.api.radio-browser.info"
        );
        assert_eq!(config.radio_dir, PathBuf::from("/data/radio"));

        let no_search = load(&[("AUTOREVERSE_RADIO_DIRECTORY", "0")]);
        assert!(no_search.radio && !no_search.radio_directory);
        // Without the feature there is no search either, whatever its own switch says.
        let off = load(&[
            ("AUTOREVERSE_RADIO", " off "),
            ("AUTOREVERSE_RADIO_DIRECTORY", "1"),
        ]);
        assert!(!off.radio && !off.radio_directory);

        let mirror = load(&[("AUTOREVERSE_RADIO_DIRECTORY_URL", "http://127.0.0.1:4000/")]);
        assert_eq!(mirror.radio_directory_url, "http://127.0.0.1:4000");
    }

    #[test]
    fn loudness_is_measured_unless_switched_off() {
        let config = load(&[]);
        assert!(config.loudness);
        assert!((1..=4).contains(&config.loudness_threads));
        assert!(!load(&[("AUTOREVERSE_LOUDNESS", "0")]).loudness);
        assert_eq!(
            load(&[("AUTOREVERSE_LOUDNESS_THREADS", " 3 ")]).loudness_threads,
            3
        );
        // Zero threads would measure nothing; that is what AUTOREVERSE_LOUDNESS=0 is for.
        for raw in ["0", "viele", "-2", ""] {
            let config = load(&[("AUTOREVERSE_LOUDNESS_THREADS", raw)]);
            assert!((1..=4).contains(&config.loudness_threads), "{raw:?}");
        }
    }

    #[test]
    fn empty_admin_values_count_as_unset() {
        let config = load(&[
            ("AUTOREVERSE_ADMIN_USER", ""),
            ("AUTOREVERSE_ADMIN_PASSWORD", "x"),
        ]);
        assert_eq!(config.admin_user, None);
        assert_eq!(config.admin_password.as_deref(), Some("x"));
    }

    #[test]
    fn origin_decides_secure_cookies() {
        assert!(!load(&[]).secure_cookies());
        assert!(!load(&[("ORIGIN", "http://nas.local:3000")]).secure_cookies());
        let config = load(&[("ORIGIN", "https://audio.example.com/")]);
        assert!(config.secure_cookies());
        assert_eq!(config.origin.as_deref(), Some("https://audio.example.com"));
    }

    #[test]
    fn static_dir_comes_from_the_environment() {
        assert_eq!(load(&[]).static_dir, PathBuf::from("static"));
        let config = load(&[("AUTOREVERSE_STATIC", "/srv/static")]);
        assert_eq!(config.static_dir, PathBuf::from("/srv/static"));
    }

    fn static_tree(root: &Path) -> PathBuf {
        let dir = root.join("static");
        std::fs::create_dir_all(dir.join("css")).unwrap();
        std::fs::write(dir.join("css/app.css"), "").unwrap();
        dir
    }

    #[test]
    fn finds_static_beside_the_binary_first() {
        let root = tempfile::tempdir().unwrap();
        let dir = static_tree(root.path());
        let exe = root.path().join("autoreverse-server");
        assert_eq!(find_static_dir(None, Some(&exe)).unwrap(), dir);
    }

    #[test]
    fn falls_back_to_the_working_directory() {
        // Tests run in the crate root, where the real static/ lives.
        let exe = Path::new("/nowhere/bin/autoreverse-server");
        assert_eq!(
            find_static_dir(None, Some(exe)).unwrap(),
            PathBuf::from("static")
        );
    }

    #[test]
    fn an_explicit_static_dir_must_hold_the_assets() {
        let root = tempfile::tempdir().unwrap();
        let dir = static_tree(root.path());
        assert_eq!(find_static_dir(Some(&dir), None).unwrap(), dir);
        let empty = root.path().join("leer");
        let err = find_static_dir(Some(&empty), None).unwrap_err();
        assert!(matches!(err, ConfigError::StaticMissing(ref tried) if tried.contains("leer")));
        assert!(
            err.to_string()
                .starts_with("Statische Dateien nicht gefunden")
        );
    }

    #[test]
    fn rejects_a_broken_port() {
        let result = Config::from_lookup(|k| (k == "PORT").then(|| "abc".to_string()));
        assert!(matches!(result, Err(ConfigError::Port(_))));
    }
}
