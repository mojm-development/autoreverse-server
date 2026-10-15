//! Playback sessions, saved positions and listening time (legacy `library/playback.ts` plus
//! `continueListening` and `listeningStats` from `queries.ts`).

use std::collections::BTreeMap;

use serde::Serialize;
use sqlx::{PgPool, Postgres};

use crate::serialize::{js_number, js_number_map_btree};

/// A random version-4 UUID, the session id (`crypto.randomUUID()`).
pub fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS random source");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub async fn open_session(
    db: &PgPool,
    user_id: i32,
    item_id: i32,
    start_position: f64,
) -> Result<String, sqlx::Error> {
    let id = new_session_id();
    sqlx::query(
        "INSERT INTO playback_sessions (id, user_id, item_id, start_position)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(item_id)
    .bind(start_position)
    .execute(db)
    .await?;
    Ok(id)
}

/// Upserts the user's position in an item. `finished: None` leaves a stored flag alone (a new row
/// starts unfinished).
pub async fn save_position<'e, E>(
    db: E,
    user_id: i32,
    item_id: i32,
    position: f64,
    finished: Option<bool>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO progress (user_id, item_id, position, finished, updated_at)
         VALUES ($1, $2, $3, coalesce($4, false), now())
         ON CONFLICT (user_id, item_id) DO UPDATE
         SET position = excluded.position,
             updated_at = excluded.updated_at,
             finished = coalesce($4, progress.finished)",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(position)
    .bind(finished)
    .execute(db)
    .await?;
    Ok(())
}

/// Closes one of the user's open sessions: saves `position` and books the distance travelled
/// since the session opened as listening time — not the wall clock, a paused player is not
/// listening. `None` if there is no such open session.
pub async fn close_session(
    db: &PgPool,
    user_id: i32,
    session_id: &str,
    position: f64,
) -> Result<Option<i32>, sqlx::Error> {
    let mut tx = db.begin().await?;
    // One statement finds and closes, so two concurrent closes cannot both book the time.
    let session: Option<(i32, f64)> = sqlx::query_as(
        "UPDATE playback_sessions SET closed_at = now()
         WHERE id = $1 AND user_id = $2 AND closed_at IS NULL
         RETURNING item_id, start_position",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((item_id, start_position)) = session else {
        return Ok(None);
    };
    save_position(&mut *tx, user_id, item_id, position, None).await?;
    let listened = (position - start_position).max(0.0);
    if listened > 0.0 {
        sqlx::query("INSERT INTO listen_events (user_id, item_id, seconds) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(item_id)
            .bind(listened)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Some(item_id))
}

/// One "Weiter hören" entry: an unfinished, present item with the total length of its tracks.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ContinueEntry {
    pub id: i32,
    pub title: String,
    pub kind: String,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub cover_path: Option<String>,
    pub position: f64,
    /// Sum of the track durations; 0 without tracks, never null.
    pub duration: f64,
}

/// The user's unfinished items, most recently played first.
pub async fn continue_listening(
    db: &PgPool,
    user_id: i32,
    limit: i64,
) -> Result<Vec<ContinueEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT item.id, item.title, item.kind, item.author, item.artist,
                item.series, item.series_index, item.cover_path, progress.position,
                coalesce(total.duration, 0)::double precision AS duration
         FROM progress
         JOIN items AS item ON item.id = progress.item_id
         LEFT JOIN LATERAL (
             SELECT sum(tracks.duration) AS duration FROM tracks WHERE tracks.item_id = item.id
         ) AS total ON true
         WHERE progress.user_id = $1 AND progress.finished = false
           AND item.missing_since IS NULL
         ORDER BY progress.updated_at DESC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Listening time per UTC day over the last `days` days, plus the total. Days without
/// listening are absent rather than zero.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListeningStats {
    #[serde(serialize_with = "js_number")]
    pub total_seconds: f64,
    #[serde(serialize_with = "js_number_map_btree")]
    pub days: BTreeMap<String, f64>,
}

pub async fn listening_stats(
    db: &PgPool,
    user_id: i32,
    days: i32,
) -> Result<ListeningStats, sqlx::Error> {
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day,
                sum(seconds)::double precision AS seconds
         FROM listen_events
         WHERE user_id = $1 AND at >= now() - make_interval(days => $2)
         GROUP BY 1",
    )
    .bind(user_id)
    .bind(days)
    .fetch_all(db)
    .await?;
    let total_seconds = rows.iter().map(|(_, s)| s).sum();
    Ok(ListeningStats {
        total_seconds,
        days: rows.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_v4_uuids() {
        let id = new_session_id();
        assert_eq!(id.len(), 36);
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
        assert_ne!(id, new_session_id());
    }
}
