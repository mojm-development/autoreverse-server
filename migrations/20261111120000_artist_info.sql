-- What the wider world knows about an artist: biographies by language, a few facts, where their
-- pictures can be fetched and their most played tracks. It comes from TheAudioDB and Apple's
-- music catalogue, so answers are kept and only asked for again after a few weeks. `found` is
-- false when neither knew the name; such a row is asked for again much sooner.
CREATE TABLE artist_info (
	artist text PRIMARY KEY,
	found boolean NOT NULL,
	info jsonb NOT NULL,
	fetched_at timestamptz NOT NULL DEFAULT now()
);
