-- Out of the box the newest five episodes of every podcast stay downloaded, so that a tap in the
-- car starts at once instead of waiting for the server to fetch the file first. The default used
-- to be 0 ("nothing until someone plays it"). A stored 0 cannot be told apart from that old
-- default, so it moves to 5 as well; whoever really wants nothing sets 0 again under
-- Podcasts → Einstellungen, and a podcast's own number (items.keep_episodes) is left alone.
ALTER TABLE library_config ALTER COLUMN podcast_keep_episodes SET DEFAULT 5;
UPDATE library_config SET podcast_keep_episodes = 5 WHERE podcast_keep_episodes = 0;
