-- Lyrics: whether a track has any, so a client shows its lyrics button only where there is
-- something to show. The text itself is read from the files on demand and never stored.
-- `lyrics_embedded` is what the scan found inside the audio file; NULL means nobody has looked
-- yet (every track from before this migration, and a file that changed since). `has_lyrics` is
-- that or a .lrc/.txt beside the file, which the scan checks every time because a text added
-- later does not change the audio file.
ALTER TABLE tracks ADD COLUMN has_lyrics boolean NOT NULL DEFAULT false;
ALTER TABLE tracks ADD COLUMN lyrics_embedded boolean;

-- Listening time per track ("most played tracks"). NULL for everything booked before, and for
-- time a client reports for an item as a whole. A track's row survives a rescan as long as its
-- file keeps its path (the scanner upserts by path); when the row does go, the time stays with
-- the item.
ALTER TABLE listen_events ADD COLUMN track_id integer REFERENCES tracks (id) ON DELETE SET NULL;
-- For the foreign key itself: removing a track must not read the whole ledger.
CREATE INDEX listen_event_track ON listen_events (track_id) WHERE track_id IS NOT NULL;

-- Every statistic reads one user's events of a time range and adds up seconds per item or
-- track. With the three columns in the index that is an index-only scan, also over a ledger of
-- hundreds of thousands of rows. It replaces the plain (user_id, at) index it starts with.
CREATE INDEX listen_event_user_at_cover ON listen_events (user_id, at)
	INCLUDE (item_id, track_id, seconds);
DROP INDEX listen_event_user_at;
