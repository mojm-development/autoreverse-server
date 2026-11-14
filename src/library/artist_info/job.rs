//! Fetches what the catalogues know about every artist of the library ahead of time, so that an
//! artist's page finds it kept instead of waiting for them.
//!
//! A pass asks about every artist that has nothing kept yet or whose answer is older than
//! `AUTOREVERSE_ARTIST_INFO_DAYS` — album artists first, then the artists of tracks by how many
//! tracks they have — and fetches the pictures of album artists that are not stored yet. The
//! catalogues are free and limit how often they may be asked, so a pass goes slowly
//! ([`PACE`]) and pauses when they stop answering. The first pass begins shortly after the
//! server starts and may take hours on a large library; later ones only see what has become due.

use std::time::Duration;

use sqlx::PgPool;

use super::{fetch_missing_pictures, is_album_artist, keep_days, renew, stored};
use crate::state::AppState;

/// Not at once: the server has other things to do right after starting.
const START_DELAY: Duration = Duration::from_secs(120);
/// How often the job looks for artists that have become due.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Between two artists. Each takes one request to TheAudioDB and two to Apple, which allows
/// about twenty a minute.
pub const PACE: Duration = Duration::from_secs(6);
/// After this many artists in a row could not be asked about, the catalogues are left alone for
/// [`BACK_OFF`].
const FAILURES_BEFORE_BACKING_OFF: usize = 5;
const BACK_OFF: Duration = Duration::from_secs(15 * 60);

/// What a pass did, for the log and the tests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pass {
    pub found: usize,
    pub unknown: usize,
    pub failed: usize,
    pub pictures: usize,
}

/// Spawns the job; returns at once.
pub fn start(state: AppState) {
    if !state.config.artist_info {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(START_DELAY).await;
        loop {
            match pass(&state, PACE, BACK_OFF).await {
                Ok(done) if done != Pass::default() => tracing::info!(
                    "[artist-info] {} Interpreten gefunden, {} unbekannt, {} nicht erreichbar, {} Bilder geholt",
                    done.found,
                    done.unknown,
                    done.failed,
                    done.pictures
                ),
                Ok(_) => {}
                Err(e) => tracing::error!("[artist-info] Durchgang fehlgeschlagen: {e}"),
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// The artists of the library whose answers are due, in the order they are asked about: album
/// artists, then artists of tracks by how many tracks they have.
pub async fn due(db: &PgPool, refresh_days: f64) -> Result<Vec<String>, sqlx::Error> {
    let found_days = keep_days(true, refresh_days).unwrap_or(0.0);
    let unknown_days = keep_days(false, refresh_days).unwrap_or(0.0);
    sqlx::query_scalar(
        "WITH names AS (
             SELECT artist, 0 AS rank, 0::bigint AS tracks
             FROM items
             WHERE kind = 'album' AND artist IS NOT NULL AND artist <> ''
               AND missing_since IS NULL AND duplicate_of IS NULL
             GROUP BY artist
             UNION ALL
             SELECT track.artist, 1, count(*)
             FROM tracks AS track JOIN items AS item ON item.id = track.item_id
             WHERE item.kind = 'album' AND track.artist IS NOT NULL AND track.artist <> ''
               AND item.missing_since IS NULL AND item.duplicate_of IS NULL
             GROUP BY track.artist
         ), best AS (
             SELECT artist, min(rank) AS rank, max(tracks) AS tracks FROM names GROUP BY artist
         )
         SELECT best.artist
         FROM best LEFT JOIN artist_info AS kept ON kept.artist = best.artist
         WHERE kept.artist IS NULL
            OR ($1 > 0 AND kept.fetched_at < now() - make_interval(
                    secs => CASE WHEN kept.found THEN $1 ELSE $2 END * 86400))
         ORDER BY best.rank, best.tracks DESC, best.artist",
    )
    .bind(found_days)
    .bind(unknown_days)
    .fetch_all(db)
    .await
}

/// One pass: every artist that is due, then the missing pictures of every album artist.
pub async fn pass(
    state: &AppState,
    pace: Duration,
    back_off: Duration,
) -> Result<Pass, sqlx::Error> {
    let mut done = Pass::default();
    let mut failures_in_a_row = 0;
    for artist in due(&state.db, state.config.artist_info_days).await? {
        // A page may have renewed it meanwhile.
        if state.artist_info_renewing.claim(&artist) {
            let kept = stored(&state.db, &artist).await;
            let renewed = match kept {
                Ok(kept) => renew(state, &artist, kept).await,
                Err(e) => Err(e),
            };
            state.artist_info_renewing.release(&artist);
            match renewed? {
                Some(info) => {
                    failures_in_a_row = 0;
                    if info.found {
                        done.found += 1;
                    } else {
                        done.unknown += 1;
                    }
                }
                None => {
                    done.failed += 1;
                    failures_in_a_row += 1;
                    if failures_in_a_row >= FAILURES_BEFORE_BACKING_OFF {
                        tracing::warn!(
                            "[artist-info] Kataloge antworten nicht, Pause für {} Minuten",
                            back_off.as_secs() / 60
                        );
                        tokio::time::sleep(back_off).await;
                        failures_in_a_row = 0;
                    }
                }
            }
        }
        if is_album_artist(&state.db, &artist).await? {
            done.pictures += fetch_missing_pictures(state, &artist).await?;
        }
        tokio::time::sleep(pace).await;
    }
    // Pictures of album artists whose answers were kept already, or failed to load last time.
    let album_artists: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT kept.artist FROM artist_info AS kept
         JOIN items ON items.artist = kept.artist
         WHERE kept.found AND items.kind = 'album'
           AND items.missing_since IS NULL AND items.duplicate_of IS NULL",
    )
    .fetch_all(&state.db)
    .await?;
    for artist in album_artists {
        done.pictures += fetch_missing_pictures(state, &artist).await?;
    }
    Ok(done)
}
