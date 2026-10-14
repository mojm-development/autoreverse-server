//! `ILIKE` patterns from user input (legacy `library/like.ts`).

/// Escapes the LIKE wildcards; backslash first, so the escapes themselves survive.
pub fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// `%text%`, for `ILIKE … ESCAPE '\'`.
pub fn like_pattern(q: &str) -> String {
    format!("%{}%", escape_like(q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_backslash_first_then_percent_and_underscore() {
        assert_eq!(escape_like("a\\b%c_d"), "a\\\\b\\%c\\_d");
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(escape_like("billion"), "billion");
        assert_eq!(like_pattern("x_y"), "%x\\_y%");
    }
}
