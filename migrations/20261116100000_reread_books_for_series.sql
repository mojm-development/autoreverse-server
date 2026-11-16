-- The series detection learnt the author folder as a series ("TKKG/TKKG - 131_Titel"), "Fall" as a
-- volume word and "_" as a separator. An unchanged folder is never read again and only fills an
-- empty series, so books already scanned would keep the split shelves. Forgetting the recorded
-- file stats of book tracks makes the next scan read every book once more; locked fields stay as
-- they are (scanner::store).
UPDATE tracks
SET mtime = NULL
WHERE item_id IN (SELECT id FROM items WHERE kind = 'book');
