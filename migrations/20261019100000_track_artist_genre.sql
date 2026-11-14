-- A track's own artist (a sampler's songs each have theirs) and an album's genre, both from the
-- tags. Forgetting the music files' mtime makes the next scan read their tags again to fill them.
ALTER TABLE tracks ADD COLUMN artist text;
ALTER TABLE items ADD COLUMN genre text;
CREATE INDEX item_genre ON items (lower(genre)) WHERE genre IS NOT NULL;
UPDATE tracks SET mtime = NULL
WHERE item_id IN (SELECT id FROM items WHERE kind = 'album');
