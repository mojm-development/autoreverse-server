//! Wire-format helpers shared by the API serializers.

use chrono::{DateTime, SecondsFormat, Utc};

/// ISO-8601 in UTC with whole seconds and a `Z`: `2026-10-14T18:22:57Z`. Legacy cut the
/// milliseconds off `toISOString()` because strict ISO-8601 clients (the iOS app) reject them.
pub fn to_iso(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// `serde(serialize_with)` adapter for `Option<DateTime<Utc>>` fields.
pub fn iso_opt<S: serde::Serializer>(d: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match d {
        Some(d) => s.serialize_str(&to_iso(*d)),
        None => s.serialize_none(),
    }
}

/// `serde(serialize_with)` adapter for `DateTime<Utc>` fields.
pub fn iso<S: serde::Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_iso(*d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn drops_fractional_seconds() {
        let d = Utc.timestamp_millis_opt(1_792_002_177_123).unwrap();
        assert_eq!(to_iso(d), "2026-10-14T18:22:57Z");
    }

    #[test]
    fn whole_seconds_stay_whole() {
        let d = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(to_iso(d), "2026-01-02T03:04:05Z");
    }
}
