//! Which build is running: the crate version plus the commit the image was built from.
//!
//! The version in `Cargo.toml` rarely changes, so on its own it cannot tell an old build from a
//! new one. The Docker build stamps the commit into the binary (`AUTOREVERSE_COMMIT`,
//! `AUTOREVERSE_COMMIT_DATE`); a build without the stamp names the version only.

use crate::VERSION;

/// The commit this binary was built from, seven characters; `None` for an unstamped build.
pub fn commit() -> Option<&'static str> {
    short_commit(option_env!("AUTOREVERSE_COMMIT"))
}

/// The day of that commit (`YYYY-MM-DD`).
pub fn commit_date() -> Option<&'static str> {
    day(option_env!("AUTOREVERSE_COMMIT_DATE"))
}

/// "0.1.0 · 9465db2 · 2026-11-25", as the clients write theirs.
pub fn label() -> String {
    join(VERSION, commit(), commit_date())
}

fn short_commit(stamp: Option<&str>) -> Option<&str> {
    stamp
        .filter(|s| !s.is_empty())
        .map(|s| s.get(..7).unwrap_or(s))
}

fn day(stamp: Option<&str>) -> Option<&str> {
    stamp
        .filter(|s| !s.is_empty())
        .map(|s| s.get(..10).unwrap_or(s))
}

fn join(version: &str, commit: Option<&str>, date: Option<&str>) -> String {
    [Some(version), commit, date]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamped_commit_is_shortened() {
        assert_eq!(
            short_commit(Some("9465db2f0c1d4e5a9b8c7d6e5f4a3b2c1d0e9f8a")),
            Some("9465db2")
        );
        assert_eq!(short_commit(Some("9465db2")), Some("9465db2"));
    }

    #[test]
    fn an_empty_stamp_counts_as_none() {
        assert_eq!(short_commit(Some("")), None);
        assert_eq!(short_commit(None), None);
        assert_eq!(day(Some("")), None);
    }

    #[test]
    fn a_commit_timestamp_is_cut_to_the_day() {
        assert_eq!(day(Some("2026-11-25T21:40:12+01:00")), Some("2026-11-25"));
        assert_eq!(day(Some("2026-11-25")), Some("2026-11-25"));
    }

    #[test]
    fn the_label_names_what_is_known() {
        assert_eq!(
            join("0.1.0", Some("9465db2"), Some("2026-11-25")),
            "0.1.0 · 9465db2 · 2026-11-25"
        );
        assert_eq!(join("0.1.0", None, None), "0.1.0");
    }
}
