//! Episode chapters (legacy `podcasts/chapters.ts`): from the feed (Podlove), from the downloaded
//! file, or from a Podcasting 2.0 JSON document.

use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterMark {
    pub start: f64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterRow {
    pub position: i32,
    pub title: String,
    pub start: f64,
    pub end: f64,
}

/// No source gives us chapter ends — only where each one starts.
const OPEN_END: f64 = 3600.0;

pub fn to_chapter_rows(marks: &[ChapterMark], total_duration: f64) -> Vec<ChapterRow> {
    let mut sorted: Vec<&ChapterMark> = marks.iter().filter(|m| m.start >= 0.0).collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    sorted
        .iter()
        .enumerate()
        .map(|(index, mark)| {
            // The last chapter runs to the end of the episode. Before the file is downloaded its
            // real length is unknown, so leave an hour rather than a zero-length chapter the
            // player can never be inside of.
            let end = match sorted.get(index + 1) {
                Some(next) => next.start,
                None if total_duration > mark.start => total_duration,
                None => mark.start + OPEN_END,
            };
            ChapterRow {
                position: index as i32 + 1,
                title: if mark.title.is_empty() {
                    format!("Kapitel {}", index + 1)
                } else {
                    mark.title.clone()
                },
                start: mark.start,
                end,
            }
        })
        .collect()
}

/// Replaces the chapters of `item_id`; returns how many were stored.
pub async fn save_episode_chapters(
    db: &PgPool,
    item_id: i32,
    marks: &[ChapterMark],
    total_duration: f64,
) -> Result<usize, sqlx::Error> {
    let rows = to_chapter_rows(marks, total_duration);
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM chapters WHERE item_id = $1")
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    for row in &rows {
        sqlx::query(
            "INSERT INTO chapters (item_id, position, title, start, \"end\") VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(item_id)
        .bind(row.position)
        .bind(&row.title)
        .bind(row.start)
        .bind(row.end)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(rows.len())
}

/// JS `Number(value)` for the JSON values a chapter document may carry.
fn js_number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) if s.trim().is_empty() => 0.0,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Null) => 0.0,
        _ => f64::NAN,
    }
}

/// The Podcasting 2.0 chapter document: `{ "chapters": [{ "startTime": 0, "title": … }] }`.
/// Entries flagged `toc: false` are markers for the player, not chapters.
pub fn parse_chapters_json(text: &str) -> Vec<ChapterMark> {
    let Ok(parsed) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = parsed.get("chapters").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(Value::as_object)
        .filter(|entry| entry.get("toc") != Some(&Value::Bool(false)))
        .filter_map(|entry| {
            let start = js_number(entry.get("startTime"));
            if !start.is_finite() || start < 0.0 {
                return None;
            }
            let title = entry
                .get("title")
                .and_then(Value::as_str)
                .map(|t| t.trim().to_owned())
                .unwrap_or_default();
            Some(ChapterMark { start, title })
        })
        .collect()
}

/// Chapters are a bonus; a feed that cannot serve them must not fail a download.
pub async fn fetch_chapters_json(http: &reqwest::Client, url: &str) -> Vec<ChapterMark> {
    let response = http.get(url).timeout(Duration::from_secs(10)).send().await;
    match response {
        Ok(r) if r.status().is_success() => match r.text().await {
            Ok(text) => parse_chapters_json(&text),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(start: f64, title: &str) -> ChapterMark {
        ChapterMark {
            start,
            title: title.into(),
        }
    }

    #[test]
    fn closes_each_chapter_where_the_next_starts_the_last_at_the_end() {
        assert_eq!(
            to_chapter_rows(&[mark(750.0, "B"), mark(0.0, "A")], 1800.0),
            vec![
                ChapterRow {
                    position: 1,
                    title: "A".into(),
                    start: 0.0,
                    end: 750.0
                },
                ChapterRow {
                    position: 2,
                    title: "B".into(),
                    start: 750.0,
                    end: 1800.0
                },
            ]
        );
    }

    #[test]
    fn leaves_the_last_chapter_open_while_the_length_is_unknown() {
        let rows = to_chapter_rows(&[mark(0.0, "A")], 0.0);
        assert!(rows[0].end > rows[0].start);
    }

    #[test]
    fn names_an_untitled_chapter() {
        assert_eq!(
            to_chapter_rows(&[mark(0.0, "")], 10.0)[0].title,
            "Kapitel 1"
        );
    }

    #[test]
    fn reads_the_json_document_and_skips_non_toc_markers() {
        let json = serde_json::json!({
            "version": "1.2.0",
            "chapters": [
                { "startTime": 0, "title": "Intro" },
                { "startTime": 62.5, "title": "Thema" },
                { "startTime": 100, "title": "Werbung", "toc": false },
                { "startTime": "kaputt", "title": "Ungültig" }
            ]
        })
        .to_string();
        assert_eq!(
            parse_chapters_json(&json),
            vec![mark(0.0, "Intro"), mark(62.5, "Thema")]
        );
    }

    #[test]
    fn returns_nothing_for_a_document_that_is_not_chapters() {
        assert!(parse_chapters_json("nicht json").is_empty());
        assert!(parse_chapters_json(r#"{"foo":1}"#).is_empty());
    }
}
