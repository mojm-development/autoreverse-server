//! Loudness values a tagging tool already wrote into a file: ReplayGain (Vorbis comments, ID3
//! `TXXX`, APEv2, MP4 free-form atoms) and Opus' `R128_*` gains.
//!
//! Everything is brought to one scale: the gain towards [`TARGET_LUFS`]. iTunes' `iTunNORM` is
//! deliberately not read — its ten hex words mix two measurements and a peak in units that
//! differ between iTunes versions, and a wrong guess is worse than measuring the file.

use std::path::Path;

use super::TARGET_LUFS;

/// What the tags of one file say. `None` where they say nothing usable.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoudnessTags {
    /// dB towards [`TARGET_LUFS`].
    pub track_gain: Option<f64>,
    pub album_gain: Option<f64>,
    /// Linear, 1.0 = full scale.
    pub track_peak: Option<f64>,
}

impl LoudnessTags {
    /// Whether nothing is left to measure for this file.
    pub fn complete(&self) -> bool {
        self.track_gain.is_some() && self.album_gain.is_some()
    }
}

/// ReplayGain 1 measured against pink noise at 89 dB SPL, which ReplayGain 2 defines as
/// -18 LUFS. A file that names another reference is shifted to it.
const REFERENCE_SPL: f64 = 89.0;
/// Opus' `R128_*` gains aim at EBU R128's -23 LUFS (RFC 7845, 5.2.1).
const R128_LUFS: f64 = -23.0;
/// Beyond this a "gain" is a broken tag, not a quiet record.
const MAX_GAIN_DB: f64 = 60.0;
/// A reference further than this from the usual one is not believed either.
const MAX_REFERENCE_SHIFT: f64 = 12.0;

/// "-6.48 dB", "+1,5 dB", "-6.48", "-7 LU" → the number.
fn number_with_unit(raw: &str, units: &[&str]) -> Option<f64> {
    let lower = raw.trim().to_lowercase();
    let mut text = lower.as_str();
    for unit in units {
        if let Some(rest) = text.strip_suffix(unit) {
            text = rest;
            break;
        }
    }
    let text = text.trim().replace(',', ".");
    let text = text.strip_prefix('+').unwrap_or(&text);
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub fn parse_gain(raw: &str) -> Option<f64> {
    number_with_unit(raw, &["db", "lu"]).filter(|gain| gain.abs() <= MAX_GAIN_DB)
}

/// A linear peak. Zero is what some tools write for "not measured", and it would allow any boost.
pub fn parse_peak(raw: &str) -> Option<f64> {
    number_with_unit(raw, &[]).filter(|peak| *peak > 0.0 && *peak <= 64.0)
}

/// Opus stores the gain as a Q7.8 fixed-point integer ("-1280" = -5 dB) towards -23 LUFS.
pub fn parse_r128(raw: &str) -> Option<f64> {
    let fixed: i32 = raw.trim().parse().ok()?;
    (-32768..=32767)
        .contains(&fixed)
        .then(|| f64::from(fixed) / 256.0 + (TARGET_LUFS - R128_LUFS))
}

/// What to add to a file's gains so that they aim at [`TARGET_LUFS`]:
/// `REPLAYGAIN_REFERENCE_LOUDNESS` is "89.0 dB" for most tools, "-18.00 LUFS" for some, and
/// whatever the user asked for with others (loudgain can tag for -23 LUFS).
fn reference_shift(raw: &str) -> Option<f64> {
    let shift = if raw.to_lowercase().contains("lufs") {
        TARGET_LUFS - number_with_unit(raw, &["lufs"])?
    } else {
        REFERENCE_SPL - number_with_unit(raw, &["db"])?
    };
    (shift.abs() <= MAX_REFERENCE_SHIFT).then_some(shift)
}

/// The first value under `key` that `parse` accepts. Keys arrive lower-cased, some with a
/// prefix (`com.apple.itunes:replaygain_track_gain`).
fn first<T>(pairs: &[(String, String)], key: &str, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
    pairs
        .iter()
        .filter(|(name, _)| name.rsplit(':').next().unwrap_or(name) == key)
        .find_map(|(_, value)| parse(value))
}

/// Reads the loudness tags out of a file's native key/value pairs
/// (`scanner::tags::native_pairs`). ReplayGain wins over `R128_*` where a file has both: it is
/// the one with a peak beside it.
pub fn from_pairs(pairs: &[(String, String)]) -> LoudnessTags {
    let shift = first(pairs, "replaygain_reference_loudness", reference_shift).unwrap_or(0.0);
    let replaygain = |key: &str| first(pairs, key, parse_gain).map(|gain| gain + shift);
    let round = |gain: f64| (gain * 100.0).round() / 100.0;
    LoudnessTags {
        track_gain: replaygain("replaygain_track_gain")
            .or_else(|| first(pairs, "r128_track_gain", parse_r128))
            .map(round),
        album_gain: replaygain("replaygain_album_gain")
            .or_else(|| first(pairs, "r128_album_gain", parse_r128))
            .map(round),
        track_peak: first(pairs, "replaygain_track_peak", parse_peak),
    }
}

/// The loudness tags of the file at `path`; `None` if the file cannot be opened (a share that
/// is not mounted) — then nothing is known, which is not "no tags".
pub fn read(path: &Path) -> Option<LoudnessTags> {
    crate::scanner::tags::native_pairs_of(path).map(|pairs| from_pairs(&pairs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn gains_are_read_in_every_spelling() {
        assert_eq!(parse_gain("-6.48 dB"), Some(-6.48));
        assert_eq!(parse_gain("-6.48 DB"), Some(-6.48));
        assert_eq!(parse_gain("  -6,48dB "), Some(-6.48));
        assert_eq!(parse_gain("+1.5 dB"), Some(1.5));
        assert_eq!(parse_gain("2"), Some(2.0));
        assert_eq!(parse_gain("-7.1 LU"), Some(-7.1));
        assert_eq!(parse_gain(""), None);
        assert_eq!(parse_gain("dB"), None);
        assert_eq!(parse_gain("laut"), None);
        assert_eq!(parse_gain("NaN dB"), None);
        assert_eq!(parse_gain("inf"), None);
        // No record is 200 dB too loud.
        assert_eq!(parse_gain("-200 dB"), None);
    }

    #[test]
    fn peaks_are_linear_and_never_zero() {
        assert_eq!(parse_peak("0.988553"), Some(0.988553));
        assert_eq!(parse_peak("1,02"), Some(1.02));
        assert_eq!(parse_peak("0.000000"), None);
        assert_eq!(parse_peak("-0.5"), None);
        assert_eq!(parse_peak("x"), None);
    }

    #[test]
    fn r128_gains_are_fixed_point_towards_minus_23() {
        // 0 means "already at -23 LUFS": five dB to go to -18.
        assert_eq!(parse_r128("0"), Some(5.0));
        assert_eq!(parse_r128("-1280"), Some(0.0));
        assert_eq!(parse_r128(" 256 "), Some(6.0));
        assert_eq!(parse_r128("-384"), Some(3.5));
        assert_eq!(parse_r128("-6.5"), None);
        assert_eq!(parse_r128("99999"), None);
    }

    #[test]
    fn replaygain_tags_of_every_container() {
        // Vorbis comment / FLAC, ID3 TXXX, MP4 free-form, APEv2: all arrive as lower-cased keys.
        for prefix in ["", "txxx:", "----:com.apple.itunes:", "com.apple.itunes:"] {
            let found = from_pairs(&pairs(&[
                (&format!("{prefix}replaygain_track_gain"), "-6.48 dB"),
                (&format!("{prefix}replaygain_album_gain"), "-5,10 dB"),
                (&format!("{prefix}replaygain_track_peak"), "0.977"),
                (&format!("{prefix}replaygain_album_peak"), "1.0"),
            ]));
            assert_eq!(
                found,
                LoudnessTags {
                    track_gain: Some(-6.48),
                    album_gain: Some(-5.1),
                    track_peak: Some(0.977),
                },
                "{prefix:?}"
            );
            assert!(found.complete());
        }
    }

    #[test]
    fn a_track_gain_alone_is_not_complete() {
        let found = from_pairs(&pairs(&[("replaygain_track_gain", "-3 dB")]));
        assert_eq!(found.track_gain, Some(-3.0));
        assert_eq!(found.album_gain, None);
        assert!(!found.complete());
        assert_eq!(
            from_pairs(&pairs(&[("title", "x")])),
            LoudnessTags::default()
        );
    }

    #[test]
    fn opus_gains_are_converted_and_replaygain_wins() {
        let opus = from_pairs(&pairs(&[
            ("r128_track_gain", "-1792"),
            ("r128_album_gain", "-1280"),
        ]));
        assert_eq!(opus.track_gain, Some(-2.0));
        assert_eq!(opus.album_gain, Some(0.0));
        assert_eq!(opus.track_peak, None);

        let both = from_pairs(&pairs(&[
            ("r128_track_gain", "-1792"),
            ("replaygain_track_gain", "-4.00 dB"),
        ]));
        assert_eq!(both.track_gain, Some(-4.0));
    }

    #[test]
    fn a_broken_value_falls_through_to_the_next_one() {
        let found = from_pairs(&pairs(&[
            ("replaygain_track_gain", "kaputt"),
            ("replaygain_track_gain", "-2.5 dB"),
        ]));
        assert_eq!(found.track_gain, Some(-2.5));
    }

    #[test]
    fn another_reference_loudness_shifts_the_gains() {
        let usual = from_pairs(&pairs(&[
            ("replaygain_reference_loudness", "89.0 dB"),
            ("replaygain_track_gain", "-6.00 dB"),
        ]));
        assert_eq!(usual.track_gain, Some(-6.0));
        let lufs = from_pairs(&pairs(&[
            ("replaygain_reference_loudness", "-18.00 LUFS"),
            ("replaygain_track_gain", "-6.00 dB"),
        ]));
        assert_eq!(lufs.track_gain, Some(-6.0));
        // Tagged for -23 LUFS: five dB more to reach -18.
        let quiet = from_pairs(&pairs(&[
            ("replaygain_reference_loudness", "-23.00 LUFS"),
            ("replaygain_track_gain", "-11.00 dB"),
            ("replaygain_album_gain", "-10.00 dB"),
        ]));
        assert_eq!(quiet.track_gain, Some(-6.0));
        assert_eq!(quiet.album_gain, Some(-5.0));
        // The old 83 dB reference of some early tools.
        let old = from_pairs(&pairs(&[
            ("replaygain_reference_loudness", "83 dB"),
            ("replaygain_track_gain", "-12.00 dB"),
        ]));
        assert_eq!(old.track_gain, Some(-6.0));
        // Nonsense is ignored rather than applied.
        let odd = from_pairs(&pairs(&[
            ("replaygain_reference_loudness", "0 dB"),
            ("replaygain_track_gain", "-6.00 dB"),
        ]));
        assert_eq!(odd.track_gain, Some(-6.0));
    }
}
