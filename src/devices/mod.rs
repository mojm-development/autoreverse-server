//! Remote control between the devices of one account ("Fernsteuerung"): who is online, what
//! plays where, commands from one device to another, handing playback over.
//!
//! The server only passes things on. Nothing here touches the database: a device tells what it
//! plays, the others are told, and a restart forgets it all — the devices reconnect and report
//! again by themselves. The wire shapes below are the contract the apps are built against (see
//! the README, "Remote control"); they only ever grow.

pub mod hub;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use hub::{Hub, Timing};

/// Longest queue a device may report: enough for a long playlist from where it stands, small
/// enough that passing it on to every other device stays cheap.
pub const MAX_QUEUE: usize = 300;
const MAX_TEXT: usize = 500;
/// Far beyond any real file (115 days), but keeps nonsense out of everyone's arithmetic.
const MAX_SECONDS: f64 = 10_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Desktop,
    Phone,
    Tablet,
    Web,
    Car,
}

impl Kind {
    pub fn parse(raw: &str) -> Option<Kind> {
        Some(match raw {
            "desktop" => Kind::Desktop,
            "phone" => Kind::Phone,
            "tablet" => Kind::Tablet,
            "web" => Kind::Web,
            "car" => Kind::Car,
            _ => return None,
        })
    }
}

/// 8–64 characters out of `[A-Za-z0-9-]`: a UUID fits, a path or a header injection does not.
pub fn valid_device_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The display name as it is kept: trimmed, 1–60 characters, nothing a terminal or a log would
/// trip over.
pub fn clean_device_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    let length = name.chars().count();
    ((1..=60).contains(&length) && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Album,
    Book,
    Episode,
    Track,
    Station,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub kind: MediaKind,
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default)]
    pub track_id: Option<i64>,
    #[serde(default)]
    pub station_id: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub item_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default)]
    pub track_id: Option<i64>,
}

/// What a device plays, as it reported it. Always sent out complete, whatever the device left
/// out; `at` always as UTC with milliseconds, so every client parses one format only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub playing: bool,
    #[serde(default)]
    pub media: Option<Media>,
    pub position: f64,
    #[serde(default)]
    pub duration: f64,
    /// `None`: this device's volume cannot be set from afar.
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat: Repeat,
    #[serde(default)]
    pub queue: Vec<QueueEntry>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(with = "at_format")]
    pub at: DateTime<Utc>,
}

/// `at` on the wire: any RFC 3339 time in, UTC with milliseconds and `Z` out. Not
/// `serialize::to_iso` — whole seconds would cost the watchers up to a second of position.
mod at_format {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(at: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::iso_millis(*at))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        let raw = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&raw)
            .map(|at| at.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

pub fn iso_millis(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn text_fits(text: Option<&str>) -> bool {
    text.is_none_or(|text| text.chars().count() <= MAX_TEXT)
}

fn id_fits(id: Option<i64>) -> bool {
    id.is_none_or(|id| (1..=i64::from(i32::MAX)).contains(&id))
}

impl State {
    /// The limits beyond the shapes serde already checked; the German text is the 422's detail.
    pub fn validate(&self) -> Result<(), &'static str> {
        let seconds = |value: f64| (0.0..=MAX_SECONDS).contains(&value);
        if !seconds(self.position) {
            return Err("position außerhalb des gültigen Bereichs");
        }
        if !seconds(self.duration) {
            return Err("duration außerhalb des gültigen Bereichs");
        }
        if self.volume.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
            return Err("volume außerhalb des gültigen Bereichs");
        }
        if self.queue.len() > MAX_QUEUE {
            return Err("queue ist zu lang (höchstens 300 Einträge)");
        }
        if self
            .queue
            .iter()
            .any(|entry| !id_fits(entry.item_id) || !id_fits(entry.track_id))
        {
            return Err("queue enthält eine ungültige Kennung");
        }
        if !text_fits(self.context.as_deref()) {
            return Err("context ist zu lang");
        }
        if let Some(media) = &self.media {
            if !id_fits(media.item_id) || !id_fits(media.track_id) || !id_fits(media.station_id) {
                return Err("media enthält eine ungültige Kennung");
            }
            if !text_fits(Some(&media.title))
                || !text_fits(media.artist.as_deref())
                || !text_fits(media.item_title.as_deref())
            {
                return Err("media enthält einen zu langen Text");
            }
        }
        Ok(())
    }

    /// Whether `self` tells the watchers anything `shown` — what they were sent last — did not:
    /// anything but the position having moved on by as much as the clock did. A jump (a seek, a
    /// speed other than 1 adding up) counts; ordinary progress does not, the watchers work that
    /// out themselves.
    pub fn differs_notably(&self, shown: &State) -> bool {
        if self.playing != shown.playing
            || self.media != shown.media
            || self.duration != shown.duration
            || self.volume != shown.volume
            || self.shuffle != shown.shuffle
            || self.repeat != shown.repeat
            || self.queue != shown.queue
            || self.context != shown.context
        {
            return true;
        }
        // Both times are the reporting device's own clock, so their difference is sound even
        // where that clock is wrong.
        let elapsed = if shown.playing {
            (self.at - shown.at).as_seconds_f64()
        } else {
            0.0
        };
        (self.position - (shown.position + elapsed)).abs() > JUMP_SECONDS
    }

    /// The state as it would be `elapsed` seconds after it was reported: the position moved on
    /// while it plays, never past the end, and `at` set to `now`. A live station has no position.
    pub fn advanced(&self, elapsed: f64, now: DateTime<Utc>) -> State {
        let mut state = self.clone();
        let live = matches!(&state.media, Some(media) if media.kind == MediaKind::Station);
        if state.playing && !live {
            state.position += elapsed.max(0.0);
            if state.duration > 0.0 {
                state.position = state.position.min(state.duration);
            }
        }
        state.at = now;
        state
    }
}

/// How far a reported position may lie from where the watchers think it is before they are
/// told. Below it, timer jitter and a slow request would cause an event per report.
const JUMP_SECONDS: f64 = 3.0;

/// A command for another device, checked: `value` is what the command takes, `null` otherwise.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Command {
    pub command: &'static str,
    pub value: Value,
    pub from: String,
}

impl Command {
    pub fn stop(from: &str) -> Command {
        Command {
            command: "stop",
            value: Value::Null,
            from: from.to_owned(),
        }
    }

    /// `Err` carries the 422's German detail.
    pub fn parse(command: &str, value: &Value, from: &str) -> Result<Command, &'static str> {
        if !valid_device_id(from) {
            return Err("from ist keine gültige Gerätekennung");
        }
        let (command, value) = match command {
            "play" => ("play", Value::Null),
            "pause" => ("pause", Value::Null),
            "toggle" => ("toggle", Value::Null),
            "next" => ("next", Value::Null),
            "previous" => ("previous", Value::Null),
            "stop" => ("stop", Value::Null),
            "seek" => match value.as_f64() {
                Some(seconds) if (0.0..=MAX_SECONDS).contains(&seconds) => ("seek", value.clone()),
                _ => return Err("seek braucht als value eine Zahl ab 0 (Sekunden)"),
            },
            "volume" => match value.as_f64() {
                Some(volume) if (0.0..=1.0).contains(&volume) => ("volume", value.clone()),
                _ => return Err("volume braucht als value eine Zahl von 0 bis 1"),
            },
            "shuffle" if value.is_boolean() => ("shuffle", value.clone()),
            "shuffle" => return Err("shuffle braucht als value true oder false"),
            "repeat" => match value.as_str() {
                Some("off" | "all" | "one") => ("repeat", value.clone()),
                _ => return Err("repeat braucht als value \"off\", \"all\" oder \"one\""),
            },
            _ => return Err("Unbekannter Befehl"),
        };
        Ok(Command {
            command,
            value,
            from: from.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(value: Value) -> State {
        serde_json::from_value(value).unwrap()
    }

    fn playing_at(position: f64, at: &str) -> State {
        state(json!({
            "playing": true, "position": position, "duration": 200, "at": at,
            "media": { "kind": "track", "item_id": 1, "track_id": 2, "title": "Lied" }
        }))
    }

    #[test]
    fn device_ids_are_uuid_like() {
        assert!(valid_device_id("3f2b8c1e-6c0a-4a57-9a3e-0d1f6b7c8d9e"));
        assert!(valid_device_id("abcdefgh"));
        for bad in [
            "",
            "short",
            "has space",
            "ümlaut-ümlaut",
            "a/b/c/d/e/f",
            &"x".repeat(65),
        ] {
            assert!(!valid_device_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_device_name("  Küche  ").as_deref(), Some("Küche"));
        assert_eq!(
            clean_device_name(&"ä".repeat(60)).map(|n| n.chars().count()),
            Some(60)
        );
        assert_eq!(clean_device_name("   "), None);
        assert_eq!(clean_device_name(&"a".repeat(61)), None);
        assert_eq!(clean_device_name("a\nb"), None);
    }

    #[test]
    fn a_state_comes_out_complete_with_at_in_utc_millis() {
        let state =
            state(json!({ "playing": false, "position": 3, "at": "2026-11-13T13:00:00+01:00" }));
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            json!({
                "playing": false, "media": null, "position": 3.0, "duration": 0.0, "volume": null,
                "shuffle": false, "repeat": "off", "queue": [], "context": null,
                "at": "2026-11-13T12:00:00.000Z"
            })
        );
    }

    #[test]
    fn limits_are_checked() {
        let base = json!({ "playing": true, "position": 1, "at": "2026-11-13T12:00:00Z" });
        let with = |key: &str, value: Value| {
            let mut body = base.clone();
            body[key] = value;
            serde_json::from_value::<State>(body).map(|state| state.validate())
        };
        assert_eq!(with("volume", json!(0.5)).unwrap(), Ok(()));
        assert!(with("volume", json!(1.5)).unwrap().is_err());
        assert!(with("position", json!(-1)).unwrap().is_err());
        assert!(with("duration", json!(1e9)).unwrap().is_err());
        assert!(
            with("queue", json!(vec![json!({ "item_id": 1 }); 301]))
                .unwrap()
                .is_err()
        );
        assert_eq!(
            with("queue", json!(vec![json!({ "item_id": 1 }); 300])).unwrap(),
            Ok(())
        );
        assert!(with("queue", json!([{ "item_id": 0 }])).unwrap().is_err());
        assert!(with("context", json!("x".repeat(501))).unwrap().is_err());
        // Shapes are serde's business.
        assert!(with("repeat", json!("always")).is_err());
        assert!(with("at", json!("gestern")).is_err());
        assert!(with("media", json!({ "kind": "film", "title": "x" })).is_err());
    }

    #[test]
    fn progress_alone_is_not_notable_but_a_jump_is() {
        let shown = playing_at(10.0, "2026-11-13T12:00:00Z");
        // Ten seconds later, ten seconds further (and a little jitter).
        assert!(!playing_at(20.4, "2026-11-13T12:00:10Z").differs_notably(&shown));
        // A seek.
        assert!(playing_at(90.0, "2026-11-13T12:00:10Z").differs_notably(&shown));
        assert!(playing_at(10.0, "2026-11-13T12:00:10Z").differs_notably(&shown));
        // Double speed adds up until it is worth telling.
        assert!(!playing_at(12.0, "2026-11-13T12:00:01Z").differs_notably(&shown));
        assert!(playing_at(30.0, "2026-11-13T12:00:10Z").differs_notably(&shown));
    }

    #[test]
    fn pause_volume_and_medium_are_notable() {
        let shown = playing_at(10.0, "2026-11-13T12:00:00Z");
        let mut paused = playing_at(10.0, "2026-11-13T12:00:00Z");
        paused.playing = false;
        assert!(paused.differs_notably(&shown));
        // Paused, nothing moves however much time passes.
        let mut later = paused.clone();
        later.at += chrono::TimeDelta::seconds(600);
        assert!(!later.differs_notably(&paused));
        let mut louder = shown.clone();
        louder.volume = Some(0.5);
        assert!(louder.differs_notably(&shown));
        let mut other = shown.clone();
        other.media.as_mut().unwrap().track_id = Some(3);
        assert!(other.differs_notably(&shown));
    }

    #[test]
    fn advancing_moves_a_playing_position_up_to_the_end() {
        let now = Utc::now();
        let playing = playing_at(10.0, "2026-11-13T12:00:00Z");
        assert_eq!(playing.advanced(5.5, now).position, 15.5);
        assert_eq!(playing.advanced(5.5, now).at, now);
        assert_eq!(playing.advanced(500.0, now).position, 200.0);
        let mut paused = playing.clone();
        paused.playing = false;
        assert_eq!(paused.advanced(5.5, now).position, 10.0);
        let mut station = playing.clone();
        station.media.as_mut().unwrap().kind = MediaKind::Station;
        assert_eq!(station.advanced(5.5, now).position, 10.0);
    }

    #[test]
    fn commands_check_their_values() {
        let from = "device-aaaa";
        let ok = |name: &str, value: Value| Command::parse(name, &value, from).map(|c| c.value);
        for plain in ["play", "pause", "toggle", "next", "previous", "stop"] {
            // A stray value is dropped rather than passed on.
            assert_eq!(ok(plain, json!(5)), Ok(Value::Null), "{plain}");
        }
        assert_eq!(ok("seek", json!(12.5)), Ok(json!(12.5)));
        assert_eq!(ok("seek", json!(0)), Ok(json!(0)));
        assert_eq!(ok("volume", json!(1)), Ok(json!(1)));
        assert_eq!(ok("shuffle", json!(true)), Ok(json!(true)));
        assert_eq!(ok("repeat", json!("one")), Ok(json!("one")));
        for (name, value) in [
            ("seek", json!(-1)),
            ("seek", json!("10")),
            ("seek", Value::Null),
            ("volume", json!(1.1)),
            ("volume", json!(true)),
            ("shuffle", json!(1)),
            ("repeat", json!("always")),
            ("dance", Value::Null),
        ] {
            assert!(ok(name, value.clone()).is_err(), "{name} {value}");
        }
        assert!(Command::parse("play", &Value::Null, "x").is_err());
    }
}
