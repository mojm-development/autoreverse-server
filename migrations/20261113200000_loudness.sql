-- Loudness normalisation ("Lautstärke angleichen"): what a client has to turn a track by to
-- reach the target loudness of -18 LUFS (the ReplayGain 2 reference).
--
-- `gain_db` and `peak` are the track's own; `album_gain_db` is the same on every track of an
-- album, so that its quiet and loud pieces keep their relation. All three stay NULL until the
-- tags or a measurement gave a value.
ALTER TABLE tracks ADD COLUMN gain_db double precision;
ALTER TABLE tracks ADD COLUMN album_gain_db double precision;
ALTER TABLE tracks ADD COLUMN peak double precision;
-- Where a track stands, so that nothing is done twice and a restart carries on:
--   NULL      nobody has looked at this version of the file yet (also: every track from before
--             this migration, and a file whose size or mtime changed — `scanner::store`)
--   'pending' the tags were read and did not give everything; an album waits to be measured
--   'done'    complete, from the tags or measured
--   'failed'  could not be decoded; not tried again until the file changes
ALTER TABLE tracks ADD COLUMN loudness_state text
	CHECK (loudness_state IN ('pending', 'done', 'failed'));
-- Both passes ask for "the tracks still to do" on every scan; once the library is through,
-- that must not read the whole table.
CREATE INDEX track_loudness_todo ON tracks (item_id)
	WHERE loudness_state IS NULL OR loudness_state = 'pending';
