-- Playlists grow up: a description, a folder, a cover of their own, and sharing with the other
-- accounts of this server.
--
-- `folder` is just a name: playlists with the same name form a folder. There is no folder table
-- and no nesting, so an empty folder cannot exist and renaming one is renaming its playlists.
ALTER TABLE playlists ADD COLUMN description text;
ALTER TABLE playlists ADD COLUMN folder text;
-- An uploaded picture under AUTOREVERSE_DATA/playlists; NULL means "none", and the clients put a
-- mosaic of the entries' covers there, as they did before.
ALTER TABLE playlists ADD COLUMN cover_path text;
-- `shared`: every account of this server may see and play it. `collaborative`: they may also
-- add, remove and move entries — it only means something while `shared` is set, but is kept
-- when sharing is switched off, so switching it on again brings back what was chosen.
ALTER TABLE playlists ADD COLUMN shared boolean NOT NULL DEFAULT false;
ALTER TABLE playlists ADD COLUMN collaborative boolean NOT NULL DEFAULT false;
-- "Von anderen geteilt" reads the shared ones of all accounts; most playlists are private.
CREATE INDEX playlist_shared ON playlists (id) WHERE shared;
