-- Listening time a client measured itself and reports after the fact (POST /me/listens: offline
-- playback, single tracks, playlists). The client names every report with a key of its own, so a
-- report sent twice — a retry after a lost answer — is booked once. Events booked by closing a
-- session carry no key.
ALTER TABLE listen_events ADD COLUMN client_key text;
CREATE UNIQUE INDEX listen_event_client_key ON listen_events (user_id, client_key)
	WHERE client_key IS NOT NULL;
