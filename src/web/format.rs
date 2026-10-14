//! Display helpers for the pages (legacy `$lib/dates.ts`, `$lib/chapterTitles.ts` and the small
//! formatters inside the Svelte pages).

/// "48 min", "1 Std 12 min" — how long a listen takes, never a bare decimal. Empty for an
/// unknown length.
pub fn human_duration(seconds: f64) -> String {
    if seconds.is_nan() || seconds <= 0.0 {
        return String::new();
    }
    if seconds < 60.0 {
        return "<1 min".into();
    }
    let hours = (seconds / 3600.0).floor() as i64;
    let minutes = ((seconds % 3600.0) / 60.0).round() as i64;
    match (hours, minutes) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} Std"),
        (h, m) => format!("{h} Std {m} min"),
    }
}

/// `1:02:03`.
pub fn format_hms(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as i64;
    format!(
        "{}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// "12.5 Std" from an hour on, "48 min" below (book detail).
pub fn format_hours(seconds: f64) -> String {
    let hours = seconds / 3600.0;
    if hours >= 1.0 {
        format!("{hours:.1} Std")
    } else {
        format!("{} min", (seconds / 60.0).round() as i64)
    }
}

/// A volume number as people write it: `3`, `3.5`.
pub fn volume(index: f64) -> String {
    if index.fract() == 0.0 {
        format!("{}", index as i64)
    } else {
        format!("{index}")
    }
}

/// JS `encodeURIComponent`: everything but `A–Z a–z 0–9 - _ . ! ~ * ' ( )` percent-encoded as
/// UTF-8, so an artist name is one path segment or one query value.
pub fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Up to two initials for a name without a picture: "Die Ärzte" → "DÄ".
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .take(2)
        .filter_map(|part| part.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

const SEPARATORS: &[char] = &[' ', '\t', '-', '–', '—', ':', ',', '.', '·', '|', '/'];

fn trim_edges(value: &str) -> String {
    value
        .trim_start_matches(|c: char| c.is_whitespace() || SEPARATORS.contains(&c))
        .trim_end_matches(|c: char| c.is_whitespace() || SEPARATORS.contains(&c))
        .trim()
        .to_owned()
}

fn is_word(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

/// Length in chars of the longest common prefix of `a` and `b` (case-insensitive), cut back to a
/// word boundary: "Kapitel 1" and "Kapitel 12" share "Kapitel ".
fn shared_prefix(a: &[char], b: &[char]) -> usize {
    let max = a.len().min(b.len());
    let mut i = 0;
    while i < max && a[i].to_lowercase().eq(b[i].to_lowercase()) {
        i += 1;
    }
    while i > 0 && is_word(a.get(i - 1).copied()) && is_word(a.get(i).copied()) {
        i -= 1;
    }
    i
}

/// Moves a cut back over one word, so the labels keep the word in front of a number.
fn back_one_word(text: &[char], cut: usize) -> usize {
    let mut i = cut;
    while i > 0 && !is_word(Some(text[i - 1])) {
        i -= 1;
    }
    let trailing_end = i;
    while i > 0 && is_word(Some(text[i - 1])) {
        i -= 1;
    }
    if i == trailing_end { 0 } else { i }
}

const MIN_ITEM_PREFIX: usize = 10;
const MIN_COMMON_PREFIX: usize = 6;
const MIN_CHAPTERS_FOR_COMMON: usize = 3;

fn slice_from(text: &[char], cut: usize) -> String {
    text.get(cut..)
        .map(|s| s.iter().collect())
        .unwrap_or_default()
}

fn is_number(label: &str) -> bool {
    let mut parts = label.splitn(2, ['.', ',']);
    let whole = parts.next().unwrap_or("");
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    digits(whole) && parts.next().is_none_or(digits)
}

/// Chapter titles out of an m4b usually repeat the book: 146 chapters all called "Ein Leben ist
/// zu wenig - Die Autobiographie, Kapitel 3". The shared beginning is dropped and only what tells
/// the chapters apart is shown — unless that would leave bare numbers or an empty label.
pub fn chapter_labels(titles: &[String], item_title: Option<&str>) -> Vec<String> {
    if titles.is_empty() {
        return Vec::new();
    }
    let mut labels: Vec<String> = titles.to_vec();

    if let Some(item_title) = item_title.filter(|t| !t.is_empty()) {
        let item: Vec<char> = item_title.chars().collect();
        let chars: Vec<Vec<char>> = titles.iter().map(|t| t.chars().collect()).collect();
        let cut = chars
            .iter()
            .map(|t| shared_prefix(t, &item))
            .min()
            .unwrap_or(0);
        if cut >= MIN_ITEM_PREFIX {
            let shortened: Vec<String> = chars
                .iter()
                .map(|t| trim_edges(&slice_from(t, cut)))
                .collect();
            // A chapter that is nothing but the book's name keeps its own title.
            if shortened.iter().all(|l| !l.is_empty()) {
                labels = shortened;
            }
        }
    }

    if labels.len() >= MIN_CHAPTERS_FOR_COMMON {
        let chars: Vec<Vec<char>> = labels.iter().map(|l| l.chars().collect()).collect();
        let mut cut = chars[0].len();
        for label in &chars[1..] {
            cut = cut.min(shared_prefix(&chars[0], label));
        }
        let numeric = |list: &[String]| list.iter().all(|l| is_number(l));
        let shorten = |cut: usize| -> Vec<String> {
            chars
                .iter()
                .map(|l| trim_edges(&slice_from(l, cut)))
                .collect()
        };
        let mut shortened = shorten(cut);
        // Bare numbers are no title: give the word before the number back and try again.
        if cut > 0 && numeric(&shortened) {
            cut = back_one_word(&chars[0], cut);
            shortened = shorten(cut);
        }
        if cut >= MIN_COMMON_PREFIX
            && !numeric(&shortened)
            && shortened.iter().all(|l| !l.is_empty())
        {
            labels = shortened;
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn encodes_like_encode_uri_component() {
        assert_eq!(encode_component("Die Ärzte"), "Die%20%C3%84rzte");
        assert_eq!(encode_component("AC/DC & Co"), "AC%2FDC%20%26%20Co");
        assert_eq!(encode_component("a-b_c.d!e~f*g'h(i)"), "a-b_c.d!e~f*g'h(i)");
    }

    #[test]
    fn initials_take_the_first_letters_of_two_words() {
        assert_eq!(initials("Die Ärzte"), "DÄ");
        assert_eq!(initials("kraftwerk"), "K");
        assert_eq!(initials("The Notwist Band"), "TN");
        assert_eq!(initials("   "), "");
    }

    #[test]
    fn human_duration_reads_as_minutes_below_the_hour_and_hours_plus_minutes_above() {
        assert_eq!(human_duration(48.0 * 60.0), "48 min");
        assert_eq!(human_duration(3600.0 + 12.0 * 60.0), "1 Std 12 min");
        assert_eq!(human_duration(7200.0), "2 Std");
    }

    #[test]
    fn human_duration_says_nothing_for_an_unknown_length() {
        assert_eq!(human_duration(0.0), "");
        assert_eq!(human_duration(-5.0), "");
        assert_eq!(human_duration(20.0), "<1 min");
    }

    #[test]
    fn hms_and_hours() {
        assert_eq!(format_hms(3723.9), "1:02:03");
        assert_eq!(format_hms(0.0), "0:00:00");
        assert_eq!(format_hours(45000.0), "12.5 Std");
        assert_eq!(format_hours(1800.0), "30 min");
        assert_eq!(volume(3.0), "3");
        assert_eq!(volume(3.5), "3.5");
    }

    #[test]
    fn drops_the_book_title_an_m4b_repeats_in_every_chapter() {
        let book = "Ein Leben ist zu wenig - Die Autobiographie (Gekürzt)";
        let titles: Vec<String> = (1..=3)
            .map(|n| format!("Ein Leben ist zu wenig - Die Autobiographie, Kapitel {n}"))
            .collect();
        assert_eq!(
            chapter_labels(&titles, Some(book)),
            strings(&["Kapitel 1", "Kapitel 2", "Kapitel 3"])
        );
    }

    #[test]
    fn drops_a_shared_beginning_even_when_the_book_title_differs() {
        let titles = strings(&[
            "Teil A - Kapitel 1",
            "Teil A - Kapitel 2",
            "Teil A - Kapitel 3",
        ]);
        assert_eq!(
            chapter_labels(&titles, Some("Ganz anderer Titel")),
            strings(&["Kapitel 1", "Kapitel 2", "Kapitel 3"])
        );
    }

    #[test]
    fn leaves_real_chapter_names_alone() {
        let titles = strings(&["Die Ankunft", "Der Sturm", "Heimkehr"]);
        assert_eq!(chapter_labels(&titles, Some("Der Schwarm")), titles);
    }

    #[test]
    fn refuses_to_reduce_chapters_to_bare_numbers() {
        let titles = strings(&["Kapitel 1", "Kapitel 12", "Kapitel 13"]);
        assert_eq!(chapter_labels(&titles, None), titles);
    }

    #[test]
    fn keeps_a_title_that_would_otherwise_vanish_entirely() {
        let titles = strings(&[
            "Das Parfum",
            "Das Parfum, Kapitel 2",
            "Das Parfum, Kapitel 3",
        ]);
        assert_eq!(chapter_labels(&titles, Some("Das Parfum")), titles);
    }

    #[test]
    fn handles_one_chapter_and_none_at_all() {
        assert_eq!(chapter_labels(&[], None), Vec::<String>::new());
        assert_eq!(
            chapter_labels(&strings(&["Nur eins"]), Some("Nur eins")),
            strings(&["Nur eins"])
        );
    }
}
