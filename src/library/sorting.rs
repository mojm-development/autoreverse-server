//! SQL sort keys (legacy `library/sorting.ts`). Both are plain SQL fragments over a column
//! expression, so they compose into `ORDER BY` clauses.

/// The derived sort key of a title: lower-cased, without a leading English article — "The Wall"
/// sorts under W, as in every music player. German articles stay: "Die drei ???" is filed under D
/// by everyone who looks for it. A sort tag in the file or a sort title set by hand wins over this.
pub fn sort_key(title: &str) -> String {
    let lower = title.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            let rest = rest.trim_start();
            if !rest.is_empty() {
                return rest.to_owned();
            }
        }
    }
    lower
}

/// Digits are zero-padded to this width, so "10" sorts after "9".
const NUMBER_WIDTH: u32 = 12;

/// A natural sort key: the lower-cased text cut into digit and non-digit chunks, digits padded.
/// "2/Zwei" sorts before "10/Zehn". Never NULL — an empty text gives `'{}'`.
pub fn natural_key(column: &str) -> String {
    format!(
        "(SELECT coalesce(
             array_agg(
                 CASE WHEN chunk[1] IS NOT NULL THEN lpad(chunk[1], {NUMBER_WIDTH}, '0') ELSE chunk[2] END
                 ORDER BY ord
             ),
             '{{}}'::text[]
         )
         FROM regexp_matches(lower({column}), '([0-9]+)|([^0-9]+)', 'g') WITH ORDINALITY AS parts(chunk, ord))"
    )
}

/// "Folge 12", "Fall 04", "Teil 3": a number introduced by a word that names an instalment.
const LABELLED: &str = "(?:^|[^[:alnum:]])(?:folge|fall|teil|kapitel|episode|band|volume|vol|nummer|nr|part|chapter)[.]?[ ]*[#]?([0-9]{1,6})";

/// "04/Der Fluch", "117 - Titel", "08. Das Phantom": a leading number with a separator after it,
/// not followed by more digits ("1.000 Meilen" is no instalment).
const LEADING: &str = "^[ ]*([0-9]{1,6})[ ]*[-/._:][ ]*(?![0-9])";

/// The instalment number in a title, or NULL.
pub fn episode_number(column: &str) -> String {
    format!(
        "coalesce(
             substring(lower({column}) from '{LABELLED}'::text)::int,
             substring(lower({column}) from '{LEADING}'::text)::int
         )"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sort_key_drops_a_leading_english_article() {
        assert_eq!(sort_key("The Wall"), "wall");
        assert_eq!(sort_key("A Night at the Opera"), "night at the opera");
        assert_eq!(sort_key("An Awesome Wave"), "awesome wave");
        assert_eq!(sort_key("Die drei ???"), "die drei ???");
        assert_eq!(sort_key("The"), "the");
        assert_eq!(sort_key("A-ha Hits"), "a-ha hits");
        assert_eq!(sort_key("Theater"), "theater");
    }
}
