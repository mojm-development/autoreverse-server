-- An album hidden as a copy of another (Einstellungen → Doppelte Alben). The files stay; the
-- copy is left out of the library lists. Should the kept album vanish, the copy shows again.
ALTER TABLE items ADD COLUMN duplicate_of integer REFERENCES items (id) ON DELETE SET NULL;
ALTER TABLE items ADD CONSTRAINT duplicate_of_other CHECK (duplicate_of <> id);
CREATE INDEX item_duplicate_of ON items (duplicate_of) WHERE duplicate_of IS NOT NULL;
