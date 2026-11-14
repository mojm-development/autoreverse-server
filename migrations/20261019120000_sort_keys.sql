-- Sort keys: a leading English article no longer counts ("The Wall" under W, see
-- library::sorting::sort_key), and an album artist's sort name from the tags is kept.
ALTER TABLE items ADD COLUMN artist_sort text;
UPDATE items SET sort_title = regexp_replace(lower(btrim(title)), '^(the|a|an)\s+(?=\S)', '')
WHERE kind <> 'episode'
  AND NOT ('sortTitle' = ANY(locked_fields));
