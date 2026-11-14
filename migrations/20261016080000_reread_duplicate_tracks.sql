-- Albums and books that hold the same recording twice (a song in two formats, loose files beside
-- a disc folder holding them again). Forgetting the files' mtime makes the next scan read them
-- again, and the scanner now keeps one file per recording.
UPDATE tracks SET mtime = NULL
WHERE item_id IN (
	SELECT item_id FROM tracks
	GROUP BY item_id, lower(btrim(title)), round(duration)
	HAVING count(*) > 1
);
