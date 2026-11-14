//! Playback sessions, saved positions and listening time (legacy `library/playback.ts` plus
//! `continueListening` and `listeningStats` from `queries.ts`).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres};

use crate::auth::preferences::SPEED_MAX;
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

/// Slack on top of the wall clock before a reported time counts as too long: rounding on a
/// session, and a client whose clock runs a little off on a report.
const SESSION_TOLERANCE_SECONDS: f64 = 60.0;
pub const REPORT_TOLERANCE_SECONDS: f64 = 300.0;
/// A session can stay open for days (a browser tab that plays a book an evening at a time), so
/// only what no listener manages is cut off.
const MAX_SESSION_SECONDS: f64 = 7.0 * 86_400.0;
/// One report is one stretch of listening.
pub const MAX_REPORT_SECONDS: f64 = 86_400.0;
/// `/me/stats` looks back a year at most; older reports would never show up anywhere.
pub const MAX_REPORT_AGE_DAYS: i64 = 365;

/// What a client claims to have heard, cut to what can be true: never negative, never more media
/// than the fastest playback speed gets through in the wall-clock time that has passed, never
/// more than `cap`. Media seconds, not wall-clock seconds — at double speed an hour of a book
/// takes half an hour, and still counts as the hour the position moved.
pub fn plausible_seconds(claimed: f64, wall_seconds: f64, tolerance: f64, cap: f64) -> f64 {
    let possible = (wall_seconds.max(0.0) + tolerance) * SPEED_MAX;
    claimed.min(possible).min(cap).max(0.0)
}

/// How the time of a session divides among the item's tracks, as the client measured it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackShare {
    pub track_id: i32,
    pub seconds: f64,
}

/// What closing a session books: seconds per track, and seconds that belong to no track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionBooking {
    pub tracks: Vec<TrackShare>,
    pub untracked: f64,
}

/// Divides a session's listening time. One rule, whatever the client sends:
///
/// the session's total is the larger of `listened` and the sum of the shares, cut to what is
/// plausible (`bound`, see [`plausible_seconds`]). The shares are booked to their tracks —
/// shortened in proportion if their sum alone is over the bound — and what is left of the
/// total is booked to the item without a track.
///
/// So `listened` above the sum leaves a rest without a track (time the client could not place),
/// `listened` below the sum is overruled by the sum (the shares are the finer measurement), and
/// no rule can ever book more than the bound. Shares for the same track are added up; `shares`
/// must already be the valid ones (tracks of the session's item, positive seconds).
pub fn divide_session(
    listened: Option<f64>,
    shares: &[TrackShare],
    bound: impl Fn(f64) -> f64,
) -> SessionBooking {
    let mut merged: Vec<TrackShare> = Vec::new();
    for share in shares {
        match merged.iter_mut().find(|m| m.track_id == share.track_id) {
            Some(known) => known.seconds += share.seconds,
            None => merged.push(*share),
        }
    }
    let sum: f64 = merged.iter().map(|s| s.seconds).sum();
    let total = bound(listened.unwrap_or(0.0).max(sum));
    if sum > total {
        let factor = total / sum;
        for share in &mut merged {
            share.seconds *= factor;
        }
    }
    SessionBooking {
        untracked: (total - sum).max(0.0),
        tracks: merged,
    }
}

/// Closes one of the user's open sessions and saves `position`. The listening time booked is
/// what the client measured (`listened`, kept within what is plausible), because only the client
/// knows what was audible. Without it — clients from before the field existed — it is the
/// distance travelled since the session opened, which is wrong as soon as someone seeks: a jump
/// ahead counts as listened to, a jump back as nothing. Never the wall clock: a paused player is
/// not listening. `None` if there is no such open session.
///
/// Without a `position` the item's saved progress stays exactly as it is — no row is made, none
/// is touched. That is for a single track out of a playlist, a mix or the radio: hearing one
/// song must not put its album under "Weiter hören" or move where the album was left. Only
/// `listened` is booked then; the distance travelled cannot be told without a position.
///
/// `shares` is the client's division of that time among the item's tracks. Shares for tracks of
/// another item are dropped; the rest is booked a row per track, see [`divide_session`]. All
/// rows of one session carry the same `at` — that is what lets a reader count the session once
/// (`library::top`). Without usable shares everything is as it was before they existed.
pub async fn close_session(
    db: &PgPool,
    user_id: i32,
    session_id: &str,
    position: Option<f64>,
    listened: Option<f64>,
    shares: &[TrackShare],
) -> Result<Option<i32>, sqlx::Error> {
    let mut tx = db.begin().await?;
    // One statement finds and closes, so two concurrent closes cannot both book the time.
    let session: Option<(i32, f64, f64)> = sqlx::query_as(
        "UPDATE playback_sessions SET closed_at = now()
         WHERE id = $1 AND user_id = $2 AND closed_at IS NULL
         RETURNING item_id, start_position,
                   extract(epoch FROM now() - started_at)::double precision",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((item_id, start_position, open_seconds)) = session else {
        return Ok(None);
    };
    if let Some(position) = position {
        save_position(&mut *tx, user_id, item_id, position, None).await?;
    }
    let bound = |claimed: f64| {
        plausible_seconds(
            claimed,
            open_seconds,
            SESSION_TOLERANCE_SECONDS,
            MAX_SESSION_SECONDS,
        )
    };
    let shares = if shares.is_empty() {
        Vec::new()
    } else {
        let claimed: Vec<i32> = shares.iter().map(|s| s.track_id).collect();
        let own: Vec<i32> =
            sqlx::query_scalar("SELECT id FROM tracks WHERE item_id = $1 AND id = ANY($2)")
                .bind(item_id)
                .bind(&claimed)
                .fetch_all(&mut *tx)
                .await?;
        shares
            .iter()
            .filter(|s| own.contains(&s.track_id))
            .copied()
            .collect()
    };
    let booking = if shares.is_empty() {
        SessionBooking {
            tracks: Vec::new(),
            untracked: match (listened, position) {
                (Some(claimed), _) => bound(claimed),
                (None, Some(position)) => (position - start_position).max(0.0),
                (None, None) => 0.0,
            },
        }
    } else {
        divide_session(listened, &shares, bound)
    };
    // One statement for all rows: `now()` is the transaction's time, so they share their `at`.
    let mut track_ids: Vec<Option<i32>> = Vec::new();
    let mut seconds: Vec<f64> = Vec::new();
    for share in booking.tracks.iter().filter(|s| s.seconds > 0.0) {
        track_ids.push(Some(share.track_id));
        seconds.push(share.seconds);
    }
    if booking.untracked > 0.0 {
        track_ids.push(None);
        seconds.push(booking.untracked);
    }
    if !seconds.is_empty() {
        sqlx::query(
            "INSERT INTO listen_events (user_id, item_id, track_id, seconds)
             SELECT $1, $2, share.track_id, share.seconds
             FROM unnest($3::int[], $4::double precision[]) AS share(track_id, seconds)",
        )
        .bind(user_id)
        .bind(item_id)
        .bind(&track_ids)
        .bind(&seconds)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Some(item_id))
}

/// Listening time measured by a client outside a session, as `POST /me/listens` takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListenReport {
    /// The client's own name for this report; the same key is booked once per user.
    pub key: String,
    pub item_id: Option<i32>,
    pub track_id: Option<i32>,
    pub seconds: f64,
    pub started_at: DateTime<Utc>,
}

/// What became of a report. Each outcome is final: sending the report again changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum ListenOutcome {
    /// Booked with this many seconds, which may be fewer than reported.
    Booked(f64),
    /// A report with this key was booked before.
    Duplicate,
    /// Not bookable, with the German reason.
    Rejected(&'static str),
}

/// Books one report. The event is dated to when the listening began, so time heard offline last
/// week shows up in last week's statistics, not on the day the phone found the server again.
pub async fn book_listen(
    db: &PgPool,
    user_id: i32,
    report: &ListenReport,
) -> Result<ListenOutcome, sqlx::Error> {
    let now = Utc::now();
    let age = (now - report.started_at).as_seconds_f64();
    if age < -REPORT_TOLERANCE_SECONDS {
        return Ok(ListenOutcome::Rejected("started_at liegt in der Zukunft"));
    }
    if age > (MAX_REPORT_AGE_DAYS * 86_400) as f64 {
        return Ok(ListenOutcome::Rejected("started_at liegt zu weit zurück"));
    }
    let seconds = plausible_seconds(
        report.seconds,
        age,
        REPORT_TOLERANCE_SECONDS,
        MAX_REPORT_SECONDS,
    );
    // A track counts for the item it belongs to — taste and most statistics are kept per item —
    // and is booked along with it, for the statistics per track.
    let item_id: Option<i32> = match (report.track_id, report.item_id) {
        (Some(track_id), claimed) => {
            let owner: Option<i32> = sqlx::query_scalar("SELECT item_id FROM tracks WHERE id = $1")
                .bind(track_id)
                .fetch_optional(db)
                .await?;
            match (owner, claimed) {
                (None, _) => return Ok(ListenOutcome::Rejected("Unbekannter Track")),
                (Some(owner), Some(claimed)) if owner != claimed => {
                    return Ok(ListenOutcome::Rejected("track_id gehört nicht zu item_id"));
                }
                (owner, _) => owner,
            }
        }
        (None, Some(item_id)) => {
            sqlx::query_scalar("SELECT id FROM items WHERE id = $1")
                .bind(item_id)
                .fetch_optional(db)
                .await?
        }
        (None, None) => return Ok(ListenOutcome::Rejected("item_id oder track_id fehlt")),
    };
    let Some(item_id) = item_id else {
        return Ok(ListenOutcome::Rejected("Unbekanntes Item"));
    };
    // The unique index decides, not a lookup beforehand: two copies of a report arriving at the
    // same moment must not both get through.
    let inserted: Option<i32> = sqlx::query_scalar(
        "INSERT INTO listen_events (user_id, item_id, seconds, at, client_key, track_id)
         VALUES ($1, $2, $3, least($4, now()), $5, $6)
         ON CONFLICT (user_id, client_key) WHERE client_key IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(seconds)
    .bind(report.started_at)
    .bind(&report.key)
    .bind(report.track_id)
    .fetch_optional(db)
    .await?;
    Ok(match inserted {
        Some(_) => ListenOutcome::Booked(seconds),
        None => ListenOutcome::Duplicate,
    })
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

/// One stretch of listening to one item, as `GET /me/history` lists it.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct HistoryRun {
    pub item_id: i32,
    pub listened_at: DateTime<Utc>,
    pub seconds: f64,
}

/// As far back as the history looks. Events are read newest first and folded into runs, and a
/// ledger that has grown for years must not be read whole for a list of fifty.
const HISTORY_EVENTS: i64 = 5000;

/// What the user heard last, newest first. Events of the same item that follow one another are
/// one entry — an album played over three sessions is one line, not three — with the seconds
/// added up and the time of the latest. Missing items are left out before the folding, so what
/// was heard either side of one still joins up.
///
/// Per item, not per track: every row of the ledger names its item, and a session that was
/// booked a row per track is still one stretch of listening to one album. The rows of one
/// session follow one another (same `at`, consecutive ids), so they fold into one entry like
/// any other run.
pub async fn listening_history(
    db: &PgPool,
    user_id: i32,
    limit: i64,
) -> Result<Vec<HistoryRun>, sqlx::Error> {
    sqlx::query_as(
        "WITH events AS (
             SELECT l.id, l.item_id, l.seconds, l.at,
                    lag(l.item_id) OVER (ORDER BY l.at DESC, l.id DESC) AS previous
             FROM (SELECT * FROM listen_events WHERE user_id = $1
                   ORDER BY at DESC, id DESC LIMIT $3) l
             JOIN items i ON i.id = l.item_id
             WHERE i.missing_since IS NULL
         ), runs AS (
             SELECT item_id, seconds, at,
                    count(*) FILTER (WHERE previous IS DISTINCT FROM item_id)
                        OVER (ORDER BY at DESC, id DESC) AS run
             FROM events
         )
         SELECT item_id, max(at) AS listened_at, sum(seconds)::double precision AS seconds
         FROM runs GROUP BY run, item_id
         ORDER BY run LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .bind(HISTORY_EVENTS)
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
    fn claimed_listening_time_is_cut_to_what_is_possible() {
        // Within the wall clock: taken as it is, also at more than single speed.
        assert_eq!(plausible_seconds(90.0, 100.0, 60.0, 1e6), 90.0);
        assert_eq!(plausible_seconds(300.0, 100.0, 60.0, 1e6), 300.0);
        // More than the fastest speed gets through in 100 s plus the slack.
        assert_eq!(plausible_seconds(5000.0, 100.0, 60.0, 1e6), 640.0);
        assert_eq!(plausible_seconds(5000.0, 100.0, 60.0, 500.0), 500.0);
        assert_eq!(plausible_seconds(-5.0, 100.0, 60.0, 1e6), 0.0);
        // A clock that ran backwards leaves the slack, not a negative bound.
        assert_eq!(plausible_seconds(1000.0, -30.0, 60.0, 1e6), 240.0);
    }

    fn share(track_id: i32, seconds: f64) -> TrackShare {
        TrackShare { track_id, seconds }
    }

    #[test]
    fn a_session_divides_into_its_tracks_and_a_rest() {
        let unbounded = |claimed: f64| claimed;
        // Shares alone: each track gets its own, nothing is left over.
        let only = divide_session(None, &[share(1, 100.0), share(2, 50.0)], unbounded);
        assert_eq!(only.tracks, [share(1, 100.0), share(2, 50.0)]);
        assert_eq!(only.untracked, 0.0);
        // `listened` above the sum: the difference has no track.
        let more = divide_session(Some(200.0), &[share(1, 100.0), share(2, 50.0)], unbounded);
        assert_eq!(more.tracks, [share(1, 100.0), share(2, 50.0)]);
        assert_eq!(more.untracked, 50.0);
        // `listened` below the sum: the shares count.
        let less = divide_session(Some(20.0), &[share(1, 100.0), share(2, 50.0)], unbounded);
        assert_eq!(less.tracks, [share(1, 100.0), share(2, 50.0)]);
        assert_eq!(less.untracked, 0.0);
        // The same track twice is one row.
        let twice = divide_session(
            None,
            &[share(1, 10.0), share(2, 5.0), share(1, 20.0)],
            unbounded,
        );
        assert_eq!(twice.tracks, [share(1, 30.0), share(2, 5.0)]);
    }

    #[test]
    fn shares_over_the_bound_shrink_in_proportion() {
        let bound = |claimed: f64| claimed.min(60.0);
        let cut = divide_session(Some(500.0), &[share(1, 100.0), share(2, 50.0)], bound);
        assert_eq!(cut.tracks, [share(1, 40.0), share(2, 20.0)]);
        assert_eq!(cut.untracked, 0.0);
        // Under the bound the rest is what the bound leaves, not what was claimed.
        let rest = divide_session(Some(500.0), &[share(1, 30.0)], bound);
        assert_eq!(rest.tracks, [share(1, 30.0)]);
        assert_eq!(rest.untracked, 30.0);
    }

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
