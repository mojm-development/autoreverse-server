//! Runtime configuration from the environment (legacy `config.ts`).
//!
//! Only process-level settings live here. The library roots are configured at runtime and stored
//! in `library_config`, not in the environment.

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
    /// Scaled-down covers and artist images, rendered on first request; safe to delete.
    pub thumbs_dir: PathBuf,
    /// 0 turns the background refresh off.
    pub podcast_refresh_hours: f64,
    /// The iTunes Search API; `AUTOREVERSE_PODCAST_DIRECTORY_URL` points it elsewhere (the e2e
    /// run serves a fixture, so no browser test reaches the internet).
    pub podcast_directory_url: String,
    /// First-admin bootstrap, only used against an empty `users` table.
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    pub host: IpAddr,
    pub port: u16,
    /// Public origin as the browser sees it (`https://audio.example.com`). Needed behind a
    /// TLS-terminating proxy: it decides the cookie's `Secure` flag and what the cross-site form
    /// check accepts as the server's own origin.
    pub origin: Option<String>,
    /// CSS, JS, icons. `AUTOREVERSE_STATIC`, else `static` (relative); the server resolves it
    /// with [`find_static_dir`] at start.
    pub static_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("PORT ist keine gültige Portnummer: {0}")]
    Port(String),
    #[error("HOST ist keine gültige IP-Adresse: {0}")]
    Host(String),
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
        Ok(Config {
            database_url: lookup("DATABASE_URL").unwrap_or_else(|| DEFAULT_DATABASE_URL.into()),
            cover_dir: data_dir.join("covers"),
            podcasts_dir: data_dir.join("podcasts"),
            artists_dir: data_dir.join("artists"),
            thumbs_dir: data_dir.join("thumbs"),
            data_dir,
            podcast_refresh_hours: refresh_hours(lookup("AUTOREVERSE_PODCAST_REFRESH_HOURS")),
            podcast_directory_url: non_empty("AUTOREVERSE_PODCAST_DIRECTORY_URL")
                .unwrap_or_else(|| crate::podcasts::directory::DIRECTORY_URL.into()),
            admin_user: non_empty("AUTOREVERSE_ADMIN_USER"),
            admin_password: non_empty("AUTOREVERSE_ADMIN_PASSWORD"),
            host,
            port,
            origin: non_empty("ORIGIN").map(|o| o.trim_end_matches('/').to_owned()),
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
fn refresh_hours(raw: Option<String>) -> f64 {
    let Some(raw) = raw else { return 1.0 };
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
    }

    #[test]
    fn artist_images_sit_beside_the_other_server_managed_files() {
        let config = load(&[("AUTOREVERSE_DATA", "/data")]);
        assert_eq!(config.artists_dir, PathBuf::from("/data/artists"));
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
