//! What a user listened to most, and the year in review — all of it read out of `listen_events`,
//! none of it by a model.
//!
//! Three things hold for every query here:
//!
//! - **One listen is counted once.** Closing a session books a row per track, all with the same
//!   `at`. For a track a listen ("play") is one such row of at least [`PLAY_SECONDS`]; for an
//!   album or an artist the rows of one moment are first added up, so an album heard in one go
//!   is one play, not one per song.
//! - **The ledger is aggregated before anything is joined to it** wherever the grouping allows:
//!   the inner query touches only `listen_events` by user and time — an index-only scan of
//!   `listen_event_user_at_cover` — and the items and tracks are looked up for the groups that
//!   come out, not for every row. (The artists are the exception, see [`top_artists`].)
//! - **A fixed number of statements**, whatever the size of the list: no query per entry.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use sqlx::PgPool;

use super::artist_covers::{chosen_covers, effective_album, fallback_covers};
use super::queries::{Item, TrackHit};

/// A listen shorter than this is skipping through, not a play.
pub const PLAY_SECONDS: f64 = 30.0;
/// "On Repeat" looks this far back, and "Repeat Rewind" at everything before.
pub const REPEAT_DAYS: i64 = 30;
/// Heard this often to count as on repeat.
pub const REPEAT_MIN_PLAYS: i32 = 2;
/// The lists of a year in review.
pub const REVIEW_TOP: i64 = 5;
/// The years a review or a top list can be asked for.
pub const MIN_YEAR: i32 = 2000;
pub const MAX_YEAR: i32 = 2100;

/// The sliding periods of `GET /me/top`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Week,
    Month,
    Year,
    All,
}

impl Period {
    pub const ALL: [Period; 4] = [Period::Week, Period::Month, Period::Year, Period::All];

    pub fn parse(raw: &str) -> Option<Period> {
        Period::ALL.into_iter().find(|p| p.key() == raw)
    }

    pub fn key(self) -> &'static str {
        match self {
            Period::Week => "week",
            Period::Month => "month",
            Period::Year => "year",
            Period::All => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Period::Week => "7 Tage",
            Period::Month => "30 Tage",
            Period::Year => "12 Monate",
            Period::All => "Insgesamt",
        }
    }

    fn days(self) -> Option<i64> {
        match self {
            Period::Week => Some(7),
            Period::Month => Some(30),
            Period::Year => Some(365),
            Period::All => None,
        }
    }
}

/// A stretch of time: from `from` (inclusive; `None` is the beginning of the ledger) up to `to`
/// (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub from: Option<DateTime<Utc>>,
    pub to: DateTime<Utc>,
}

impl Range {
    /// The last 7, 30 or 365 days up to `now`, or everything up to it.
    pub fn last(period: Period, now: DateTime<Utc>) -> Range {
        Range {
            from: period.days().map(|days| now - Duration::days(days)),
            to: now,
        }
    }

    /// A calendar year in UTC: from its first second up to the first of the next year.
    pub fn calendar_year(year: i32) -> Range {
        let start = |y: i32| Utc.with_ymd_and_hms(y, 1, 1, 0, 0, 0).unwrap();
        Range {
            from: Some(start(year)),
            to: start(year + 1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct TopTrack {
    #[sqlx(flatten)]
    pub track: TrackHit,
    pub seconds: f64,
    pub plays: i32,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct TopAlbum {
    #[sqlx(flatten)]
    pub item: Item,
    pub seconds: f64,
    pub plays: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopArtist {
    pub artist: String,
    pub seconds: f64,
    pub plays: i32,
    /// Whether `GET /artists/{artist}/image` has a picture for exactly this name.
    pub has_image: bool,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct TopGenre {
    pub genre: String,
    pub seconds: f64,
}

/// The columns a [`TrackHit`] is read from, for tracks `t` of items `i`.
const TRACK_HIT: &str = "t.id, t.title, t.artist, t.duration, t.item_id, \
                         i.title AS item_title, i.kind AS item_kind, t.has_lyrics, \
                         t.gain_db, t.album_gain_db, t.peak";

/// The music tracks heard longest in `range`. Only time that was booked to a track counts —
/// what older clients booked to an album as a whole shows up under albums, artists and genres.
pub async fn top_tracks(
    db: &PgPool,
    user_id: i32,
    range: &Range,
    limit: i64,
) -> Result<Vec<TopTrack>, sqlx::Error> {
    let sql = format!(
        "SELECT {TRACK_HIT}, h.seconds, h.plays
         FROM (SELECT track_id, sum(seconds)::double precision AS seconds,
                      (count(*) FILTER (WHERE seconds >= $5))::int AS plays
               FROM listen_events
               WHERE user_id = $1 AND at >= coalesce($2, '-infinity'::timestamptz) AND at < $3
                 AND track_id IS NOT NULL
               GROUP BY track_id) h
         JOIN tracks t ON t.id = h.track_id
         JOIN items i ON i.id = t.item_id
         WHERE i.kind = 'album' AND i.missing_since IS NULL
         ORDER BY h.seconds DESC, h.plays DESC, t.id LIMIT $4"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(range.from)
        .bind(range.to)
        .bind(limit)
        .bind(PLAY_SECONDS)
        .fetch_all(db)
        .await
}

/// The albums heard longest in `range`, whether the time was booked per track or not.
pub async fn top_albums(
    db: &PgPool,
    user_id: i32,
    range: &Range,
    limit: i64,
) -> Result<Vec<TopAlbum>, sqlx::Error> {
    sqlx::query_as(
        "SELECT i.*, h.seconds, h.plays
         FROM (SELECT item_id, sum(seconds)::double precision AS seconds,
                      (count(*) FILTER (WHERE seconds >= $5))::int AS plays
               -- A session's rows share their `at`: one play of the album, not one per track.
               FROM (SELECT item_id, sum(seconds) AS seconds FROM listen_events
                     WHERE user_id = $1 AND at >= coalesce($2, '-infinity'::timestamptz)
                       AND at < $3
                     GROUP BY item_id, at) listens
               GROUP BY item_id) h
         JOIN items i ON i.id = h.item_id
         WHERE i.kind = 'album' AND i.missing_since IS NULL
         ORDER BY h.seconds DESC, h.plays DESC, i.id LIMIT $4",
    )
    .bind(user_id)
    .bind(range.from)
    .bind(range.to)
    .bind(limit)
    .bind(PLAY_SECONDS)
    .fetch_all(db)
    .await
}

/// The artist a row of the ledger counts for: the track's own where the row names a track that
/// has one (a sampler's songs), else the album's. For events `l`, items `i`, tracks `t`.
const ARTIST_OF: &str = "coalesce(nullif(btrim(t.artist), ''), i.artist)";

/// The artists heard longest in `range`, music only. Spellings that differ only in case are
/// one artist.
pub async fn top_artists(
    db: &PgPool,
    user_id: i32,
    range: &Range,
    limit: i64,
) -> Result<Vec<TopArtist>, sqlx::Error> {
    // The one list whose grouping key is a text that has to be worked out per row. Two things
    // keep that affordable on a long ledger: the rows are grouped by a hash of the name — a
    // number — rather than by the name, and the name itself is only looked up for the entries
    // that make the list, through one row that stands for each (`rep`: its item and track).
    let sql = format!(
        "SELECT {ARTIST_OF} AS artist, top.seconds, top.plays
         FROM (SELECT key, max(rep) AS rep, sum(seconds)::double precision AS seconds,
                      (count(*) FILTER (WHERE seconds >= $5))::int AS plays
               FROM (SELECT hashtextextended(lower({ARTIST_OF}), 0) AS key,
                            max((l.item_id::bigint << 32) | coalesce(l.track_id, 0)) AS rep,
                            sum(l.seconds) AS seconds
                     FROM listen_events l
                     JOIN items i ON i.id = l.item_id
                     LEFT JOIN tracks t ON t.id = l.track_id
                     WHERE l.user_id = $1 AND l.at >= coalesce($2, '-infinity'::timestamptz)
                       AND l.at < $3
                       AND i.kind = 'album' AND i.missing_since IS NULL
                       AND {ARTIST_OF} IS NOT NULL
                     -- Per artist and moment: a session is one play however many songs it had.
                     GROUP BY 1, l.at) listens
               GROUP BY key
               ORDER BY seconds DESC, plays DESC, key LIMIT $4) top
         JOIN items i ON i.id = (top.rep >> 32)::int
         LEFT JOIN tracks t ON t.id = nullif((top.rep & 4294967295)::int, 0)
         ORDER BY top.seconds DESC, top.plays DESC, top.key"
    );
    let rows: Vec<(String, f64, i32)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(range.from)
        .bind(range.to)
        .bind(limit)
        .bind(PLAY_SECONDS)
        .fetch_all(db)
        .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    // Two more statements for the whole list, as the artists page does it.
    let (chosen, fallback) = tokio::try_join!(chosen_covers(db), fallback_covers(db))?;
    // Pictures are stored under the album artist's exact spelling. Where the name heard differs
    // from it only in case, the entry takes that spelling, so that the image address built from
    // it is the one that answers.
    let spelling: HashMap<String, &String> = chosen
        .keys()
        .chain(fallback.keys())
        .map(|name| (name.to_lowercase(), name))
        .collect();
    Ok(rows
        .into_iter()
        .map(|(heard, seconds, plays)| {
            let artist = spelling
                .get(&heard.to_lowercase())
                .map_or(heard, |known| (*known).clone());
            let choice = chosen.get(&artist);
            let has_image = choice.is_some_and(|c| c.image_path.is_some())
                || effective_album(choice, &fallback, &artist).is_some();
            TopArtist {
                artist,
                seconds,
                plays,
                has_image,
            }
        })
        .collect())
}

/// The genres heard longest in `range`: the genre of the album a row was booked to.
pub async fn top_genres(
    db: &PgPool,
    user_id: i32,
    range: &Range,
    limit: i64,
) -> Result<Vec<TopGenre>, sqlx::Error> {
    sqlx::query_as(
        "SELECT mode() WITHIN GROUP (ORDER BY i.genre) AS genre,
                sum(h.seconds)::double precision AS seconds
         FROM (SELECT item_id, sum(seconds) AS seconds FROM listen_events
               WHERE user_id = $1 AND at >= coalesce($2, '-infinity'::timestamptz) AND at < $3
               GROUP BY item_id) h
         JOIN items i ON i.id = h.item_id
         WHERE i.kind = 'album' AND i.missing_since IS NULL AND btrim(i.genre) <> ''
         GROUP BY lower(i.genre)
         ORDER BY seconds DESC, lower(i.genre) LIMIT $4",
    )
    .bind(user_id)
    .bind(range.from)
    .bind(range.to)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// "On Repeat": the tracks played most often in the last [`REPEAT_DAYS`] days, at least
/// [`REPEAT_MIN_PLAYS`] times, most played first.
pub async fn on_repeat(
    db: &PgPool,
    user_id: i32,
    limit: i64,
) -> Result<Vec<TrackHit>, sqlx::Error> {
    let sql = format!(
        "SELECT {TRACK_HIT}
         FROM (SELECT track_id, sum(seconds) AS seconds,
                      count(*) FILTER (WHERE seconds >= $3) AS plays
               FROM listen_events
               WHERE user_id = $1 AND at >= now() - make_interval(days => $5)
                 AND track_id IS NOT NULL
               GROUP BY track_id) h
         JOIN tracks t ON t.id = h.track_id
         JOIN items i ON i.id = t.item_id
         WHERE h.plays >= $4 AND i.kind = 'album' AND i.missing_since IS NULL
         ORDER BY h.plays DESC, h.seconds DESC, t.id LIMIT $2"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(limit)
        .bind(PLAY_SECONDS)
        .bind(i64::from(REPEAT_MIN_PLAYS))
        .bind(REPEAT_DAYS as i32)
        .fetch_all(db)
        .await
}

/// "Repeat Rewind": what was on repeat once — played at least [`REPEAT_MIN_PLAYS`] times more
/// than [`REPEAT_DAYS`] days ago — and has not been heard at all since.
pub async fn repeat_rewind(
    db: &PgPool,
    user_id: i32,
    limit: i64,
) -> Result<Vec<TrackHit>, sqlx::Error> {
    let sql = format!(
        "SELECT {TRACK_HIT}
         FROM (SELECT track_id, sum(seconds) AS seconds,
                      count(*) FILTER (WHERE seconds >= $3) AS plays
               FROM listen_events
               WHERE user_id = $1 AND at < now() - make_interval(days => $5)
                 AND track_id IS NOT NULL
               GROUP BY track_id) h
         JOIN tracks t ON t.id = h.track_id
         JOIN items i ON i.id = t.item_id
         WHERE h.plays >= $4 AND i.kind = 'album' AND i.missing_since IS NULL
           AND h.track_id NOT IN (
               SELECT track_id FROM listen_events
               WHERE user_id = $1 AND at >= now() - make_interval(days => $5)
                 AND track_id IS NOT NULL)
         ORDER BY h.plays DESC, h.seconds DESC, t.id LIMIT $2"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(limit)
        .bind(PLAY_SECONDS)
        .bind(i64::from(REPEAT_MIN_PLAYS))
        .bind(REPEAT_DAYS as i32)
        .fetch_all(db)
        .await
}

/// Listening time of one UTC day, by kind of media.
#[derive(Debug, Clone, Copy, PartialEq, sqlx::FromRow)]
pub struct DaySeconds {
    pub day: NaiveDate,
    pub music: f64,
    pub books: f64,
    pub podcasts: f64,
}

impl DaySeconds {
    pub fn total(&self) -> f64 {
        self.music + self.books + self.podcasts
    }
}

/// The numbers of a year that follow from its days.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct YearTotals {
    pub total: f64,
    pub music: f64,
    pub books: f64,
    pub podcasts: f64,
    pub days_listened: i32,
    /// The day with the most listening and its seconds; the earliest of equals.
    pub longest_day: Option<(NaiveDate, f64)>,
    /// The most days in a row with any listening.
    pub longest_streak_days: i32,
    /// Seconds per month, January first.
    pub months: [f64; 12],
}

/// Adds up a year from its days (`days` in ascending order, as the query returns them).
pub fn year_totals(days: &[DaySeconds]) -> YearTotals {
    let mut totals = YearTotals::default();
    let mut streak = 0;
    let mut previous: Option<NaiveDate> = None;
    for day in days.iter().filter(|d| d.total() > 0.0) {
        let seconds = day.total();
        totals.music += day.music;
        totals.books += day.books;
        totals.podcasts += day.podcasts;
        totals.days_listened += 1;
        totals.months[day.day.month0() as usize] += seconds;
        if totals.longest_day.is_none_or(|(_, most)| seconds > most) {
            totals.longest_day = Some((day.day, seconds));
        }
        streak = if previous.and_then(|p| p.succ_opt()) == Some(day.day) {
            streak + 1
        } else {
            1
        };
        totals.longest_streak_days = totals.longest_streak_days.max(streak);
        previous = Some(day.day);
    }
    totals.total = totals.music + totals.books + totals.podcasts;
    totals
}

/// `GET /me/review`: one calendar year (UTC) of a user's listening.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub year: i32,
    pub totals: YearTotals,
    pub top_tracks: Vec<TopTrack>,
    pub top_albums: Vec<TopAlbum>,
    pub top_artists: Vec<TopArtist>,
    pub top_genres: Vec<TopGenre>,
    /// Books marked finished whose position was last saved in this year, in that order.
    pub books_finished: Vec<Item>,
    /// Artists whose very first listen falls into this year.
    pub new_artists: i32,
    /// The first thing heard in the year.
    pub first_listen: Option<(DateTime<Utc>, Item)>,
}

async fn year_days(
    db: &PgPool,
    user_id: i32,
    range: &Range,
) -> Result<Vec<DaySeconds>, sqlx::Error> {
    // Every second counts here, also that of an item that has gone missing since: the time
    // was listened. Episodes and (never booked, but possible) podcasts are one kind.
    sqlx::query_as(
        "SELECT h.day,
                coalesce(sum(h.seconds) FILTER (WHERE i.kind = 'album'), 0)::double precision
                    AS music,
                coalesce(sum(h.seconds) FILTER (WHERE i.kind = 'book'), 0)::double precision
                    AS books,
                coalesce(sum(h.seconds) FILTER (WHERE i.kind IN ('episode', 'podcast')), 0)
                    ::double precision AS podcasts
         FROM (SELECT (at AT TIME ZONE 'UTC')::date AS day, item_id, sum(seconds) AS seconds
               FROM listen_events
               WHERE user_id = $1 AND at >= $2 AND at < $3
               GROUP BY 1, item_id) h
         JOIN items i ON i.id = h.item_id
         GROUP BY h.day ORDER BY h.day",
    )
    .bind(user_id)
    .bind(range.from)
    .bind(range.to)
    .fetch_all(db)
    .await
}

async fn books_finished(
    db: &PgPool,
    user_id: i32,
    range: &Range,
) -> Result<Vec<Item>, sqlx::Error> {
    // `finished_at` is kept by the database (migration `progress_finished_at`); a book begun
    // again since is no longer finished and has none.
    sqlx::query_as(
        "SELECT i.* FROM progress p JOIN items i ON i.id = p.item_id
         WHERE p.user_id = $1 AND p.finished AND i.kind = 'book' AND i.missing_since IS NULL
           AND p.finished_at >= $2 AND p.finished_at < $3
         ORDER BY p.finished_at, i.id",
    )
    .bind(user_id)
    .bind(range.from)
    .bind(range.to)
    .fetch_all(db)
    .await
}

async fn new_artists(db: &PgPool, user_id: i32, range: &Range) -> Result<i32, sqlx::Error> {
    // The one question that needs the ledger from its beginning: when an artist was heard
    // first. Aggregated per item and track before the join, so the join sees the library's
    // size, not the ledger's.
    let sql = format!(
        "SELECT count(*)::int FROM (
             SELECT min(l.first) AS first
             FROM (SELECT item_id, track_id, min(at) AS first FROM listen_events
                   WHERE user_id = $1 AND at < $3
                   GROUP BY item_id, track_id) l
             JOIN items i ON i.id = l.item_id
             LEFT JOIN tracks t ON t.id = l.track_id
             WHERE i.kind = 'album' AND {ARTIST_OF} IS NOT NULL
             GROUP BY lower({ARTIST_OF})) artists
         WHERE first >= $2"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(range.from)
        .bind(range.to)
        .fetch_one(db)
        .await
}

#[derive(sqlx::FromRow)]
struct FirstListen {
    first_at: DateTime<Utc>,
    #[sqlx(flatten)]
    item: Item,
}

async fn first_listen(
    db: &PgPool,
    user_id: i32,
    range: &Range,
) -> Result<Option<(DateTime<Utc>, Item)>, sqlx::Error> {
    let row: Option<FirstListen> = sqlx::query_as(
        "SELECT l.at AS first_at, i.* FROM listen_events l JOIN items i ON i.id = l.item_id
         WHERE l.user_id = $1 AND l.at >= $2 AND l.at < $3 AND i.missing_since IS NULL
         ORDER BY l.at, l.id LIMIT 1",
    )
    .bind(user_id)
    .bind(range.from)
    .bind(range.to)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| (row.first_at, row.item)))
}

/// A user's calendar year. A year nothing was heard in is all zeros and empty lists.
pub async fn review(db: &PgPool, user_id: i32, year: i32) -> Result<Review, sqlx::Error> {
    let range = Range::calendar_year(year);
    // In two rounds: the pool has ten connections, and one review must not take them all.
    let (days, top_tracks, top_albums, top_artists) = tokio::try_join!(
        year_days(db, user_id, &range),
        top_tracks(db, user_id, &range, REVIEW_TOP),
        top_albums(db, user_id, &range, REVIEW_TOP),
        top_artists(db, user_id, &range, REVIEW_TOP),
    )?;
    let (top_genres, books_finished, new_artists, first) = tokio::try_join!(
        top_genres(db, user_id, &range, REVIEW_TOP),
        books_finished(db, user_id, &range),
        new_artists(db, user_id, &range),
        first_listen(db, user_id, &range),
    )?;
    Ok(Review {
        year,
        totals: year_totals(&days),
        top_tracks,
        top_albums,
        top_artists,
        top_genres,
        books_finished,
        new_artists,
        first_listen: first,
    })
}

/// The calendar years (UTC) the user has any listening in, newest first.
pub async fn review_years(db: &PgPool, user_id: i32) -> Result<Vec<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT extract(year FROM at AT TIME ZONE 'UTC')::int AS year
         FROM listen_events WHERE user_id = $1 ORDER BY year DESC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(month: u32, day: u32, music: f64, books: f64, podcasts: f64) -> DaySeconds {
        DaySeconds {
            day: NaiveDate::from_ymd_opt(2026, month, day).unwrap(),
            music,
            books,
            podcasts,
        }
    }

    #[test]
    fn periods_slide_back_from_now_and_a_year_is_its_calendar() {
        let now = Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap();
        assert_eq!(
            Range::last(Period::Week, now).from,
            Some(Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap())
        );
        assert_eq!(
            Range::last(Period::Year, now).from,
            Some(Utc.with_ymd_and_hms(2025, 11, 13, 12, 0, 0).unwrap())
        );
        assert_eq!(Range::last(Period::All, now).from, None);
        assert_eq!(Range::last(Period::Month, now).to, now);
        let year = Range::calendar_year(2026);
        assert_eq!(
            year.from,
            Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
        );
        assert_eq!(year.to, Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(Period::parse("month"), Some(Period::Month));
        assert_eq!(Period::parse("decade"), None);
    }

    #[test]
    fn a_year_adds_up_from_its_days() {
        let totals = year_totals(&[
            day(1, 30, 100.0, 0.0, 0.0),
            day(1, 31, 50.0, 200.0, 0.0),
            day(2, 1, 0.0, 0.0, 250.0),
            day(2, 3, 10.0, 0.0, 0.0),
            day(12, 31, 0.0, 250.0, 0.0),
        ]);
        assert_eq!(totals.total, 860.0);
        assert_eq!(
            (totals.music, totals.books, totals.podcasts),
            (160.0, 450.0, 250.0)
        );
        assert_eq!(totals.days_listened, 5);
        // Three days in a row across the turn of the month; the earliest of two equal days.
        assert_eq!(totals.longest_streak_days, 3);
        assert_eq!(
            totals.longest_day,
            Some((NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(), 250.0))
        );
        assert_eq!(totals.months[0], 350.0);
        assert_eq!(totals.months[1], 260.0);
        assert_eq!(totals.months[11], 250.0);
        assert_eq!(totals.months[5], 0.0);
    }

    #[test]
    fn an_empty_year_is_all_zeros() {
        let totals = year_totals(&[]);
        assert_eq!(totals, YearTotals::default());
        assert_eq!(totals.longest_day, None);
        assert_eq!(totals.longest_streak_days, 0);
    }
}
