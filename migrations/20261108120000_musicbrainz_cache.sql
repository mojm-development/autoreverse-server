-- What MusicBrainz lists as an artist's studio albums. MusicBrainz allows one request per second,
-- so answers are kept and only asked for again after a few weeks.
CREATE TABLE musicbrainz_artists (
	artist text PRIMARY KEY,
	albums jsonb NOT NULL,
	fetched_at timestamptz NOT NULL DEFAULT now()
);
