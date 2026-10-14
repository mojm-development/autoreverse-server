-- Fixtures for the e2e specs, applied by run-on-jarvis.sh after the server has started (so the
-- schema is migrated and the first admin 'Oliver' is bootstrapped from AUTOREVERSE_ADMIN_USER).
-- Legacy specs inserted these through drizzle; the browser VM has no database access, so they
-- live here and the specs look the ids up by title over the API (tests/_fixtures.ts).
-- Idempotent: safe against a database that already holds them.

-- Second, non-admin user with the same password: copying Oliver's argon2id hash avoids needing a
-- hashing tool here (there is no user-management API before slice 9).
INSERT INTO users (name, password_hash, is_admin)
SELECT 'NonAdmin', password_hash, false FROM users WHERE name = 'Oliver'
ON CONFLICT (name) DO NOTHING;

CREATE TEMP TABLE e2e_items (title text, kind text, narrator text, artist text, progress double precision);
INSERT INTO e2e_items VALUES
	('E2E-Fixture-Book-WithProgress', 'book', 'A Narrator', NULL, 1234),
	('E2E-Fixture-Book-NoNarrator', 'book', NULL, NULL, NULL),
	('E2E-Fixture-Book-Restart', 'book', 'A Narrator', NULL, 500),
	('E2E-Fixture-Book-EndOfBook', 'book', 'A Narrator', NULL, 3600),
	('E2E-Fixture-NowPlaying-Book', 'book', NULL, NULL, NULL),
	('E2E-Fixture-Album-Detail', 'album', NULL, 'E2E Fixture Artist', NULL),
	-- favorites-bookmarks: a heart to toggle and a bookmark at the saved 0:12:34.
	('E2E-Fixture-Album-Favorite', 'album', NULL, 'E2E Fixture Artist', NULL),
	('E2E-Fixture-Book-Bookmark', 'book', NULL, NULL, 754);

INSERT INTO items (kind, title, sort_title, author, narrator, artist)
SELECT e.kind, e.title, lower(e.title),
	CASE WHEN e.kind = 'book' THEN 'E2E Fixture Author' END, e.narrator, e.artist
FROM e2e_items e
WHERE NOT EXISTS (SELECT 1 FROM items i WHERE i.title = e.title);

-- Paths do not exist: playback starts a session and shows the player, the stream itself 410s.
INSERT INTO tracks (item_id, position, path, duration, title)
SELECT i.id, 1, '/fixtures/' || i.title || '/01.mp3', CASE WHEN i.kind = 'album' THEN 180 ELSE 3600 END,
	CASE WHEN i.kind = 'album' THEN 'Track One' END
FROM items i JOIN e2e_items e ON e.title = i.title
ON CONFLICT (path) DO NOTHING;

INSERT INTO tracks (item_id, position, path, duration, title)
SELECT i.id, 2, '/fixtures/' || i.title || '/02.mp3', 210, 'Track Two'
FROM items i WHERE i.title = 'E2E-Fixture-Album-Detail'
ON CONFLICT (path) DO NOTHING;

INSERT INTO chapters (item_id, position, title, start, "end")
SELECT i.id, c.position, c.title, c.start, c.stop
FROM items i,
	(VALUES (1, 'Chapter 1: Beginning', 0, 1200), (2, 'Chapter 2: Middle', 1200, 2400),
		(3, 'Chapter 3: End', 2400, 3600)) AS c (position, title, start, stop)
WHERE i.title = 'E2E-Fixture-Book-EndOfBook'
ON CONFLICT (item_id, position) DO NOTHING;

-- Reset on every run: "Von vorn" and playback move these positions.
INSERT INTO progress (user_id, item_id, position, finished)
SELECT u.id, i.id, e.progress, false
FROM items i JOIN e2e_items e ON e.title = i.title, users u
WHERE u.name = 'Oliver' AND e.progress IS NOT NULL
ON CONFLICT (user_id, item_id) DO UPDATE SET position = EXCLUDED.position, finished = false;

-- secondary-pages: an artist for the /library/artists → Alben filter.
INSERT INTO items (kind, title, sort_title, artist)
SELECT 'album', 'E2E-Fixture-Album-ForArtist', 'e2e-fixture-album-forartist', 'Ansa Volt'
WHERE NOT EXISTS (SELECT 1 FROM items WHERE title = 'E2E-Fixture-Album-ForArtist');

-- search: an album whose title does not match the query, so only its track is a hit.
INSERT INTO items (kind, title, sort_title, artist)
SELECT 'album', 'Zzz-Test-Album-9182-NoMatch', 'zzz-test-album-9182-nomatch', 'Test Artist'
WHERE NOT EXISTS (SELECT 1 FROM items WHERE title = 'Zzz-Test-Album-9182-NoMatch');

INSERT INTO tracks (item_id, position, path, duration, title)
SELECT i.id, 1, '/test/paths/Zzz-Test-Track-Only-9182.mp3', 300, 'Zzz-Test-Track-Only-9182'
FROM items i WHERE i.title = 'Zzz-Test-Album-9182-NoMatch'
ON CONFLICT (path) DO NOTHING;

-- playlists: Oliver's playlist of three single tracks, each from its own album, in this order
-- (the reorder spec drags the first onto the third). Fresh on every run: the database is.
INSERT INTO items (kind, title, sort_title, artist)
SELECT 'album', t.title, lower(t.title), 'E2E Fixture Artist'
FROM (VALUES ('Erster Titel'), ('Zweiter Titel'), ('Dritter Titel'),
	('E2E-Fixture-Album-Playlist')) AS t (title)
WHERE NOT EXISTS (SELECT 1 FROM items i WHERE i.title = t.title);

INSERT INTO tracks (item_id, position, path, duration, title)
SELECT i.id, 1, '/fixtures/playlist/' || i.title || '.mp3', 200,
	CASE WHEN i.title = 'E2E-Fixture-Album-Playlist' THEN 'Track One' ELSE i.title END
FROM items i
WHERE i.title IN ('Erster Titel', 'Zweiter Titel', 'Dritter Titel', 'E2E-Fixture-Album-Playlist')
ON CONFLICT (path) DO NOTHING;

INSERT INTO playlists (user_id, name)
SELECT u.id, 'E2E-Fixture-Playlist' FROM users u
WHERE u.name = 'Oliver' AND NOT EXISTS (SELECT 1 FROM playlists WHERE name = 'E2E-Fixture-Playlist');

INSERT INTO playlist_entries (playlist_id, track_id, position)
SELECT p.id, t.id, o.position
FROM playlists p, tracks t JOIN items i ON i.id = t.item_id
	JOIN (VALUES ('Erster Titel', 1), ('Zweiter Titel', 2), ('Dritter Titel', 3)) AS o (title, position)
	ON o.title = i.title
WHERE p.name = 'E2E-Fixture-Playlist'
ON CONFLICT (playlist_id, position) DO NOTHING;
