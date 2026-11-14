-- When an item was finished. `progress` kept only the time of its last change, and a finished
-- book whose position is saved again (a client syncing, somebody scrubbing through the last
-- chapter) moved its "finished" date with it — the year in review counted it in the wrong year.
--
-- Rows finished before this column existed get that last change, the best there is.
ALTER TABLE progress ADD COLUMN finished_at timestamptz;
UPDATE progress SET finished_at = updated_at WHERE finished;

-- Kept by the database, so that every writer — the progress routes, the scan moving progress
-- between albums, the duplicate check, a seed — gets it right without knowing of it:
-- finishing sets it, unfinishing clears it, and anything else leaves it as it was. A writer
-- that carries a date along (moving progress to another item) sets it itself.
CREATE FUNCTION progress_finished_at() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
	IF NOT NEW.finished THEN
		NEW.finished_at := NULL;
	ELSIF NEW.finished_at IS NULL THEN
		NEW.finished_at := CASE
			WHEN TG_OP = 'UPDATE' AND OLD.finished THEN coalesce(OLD.finished_at, now())
			ELSE now()
		END;
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER progress_finished_at BEFORE INSERT OR UPDATE ON progress
	FOR EACH ROW EXECUTE FUNCTION progress_finished_at();
