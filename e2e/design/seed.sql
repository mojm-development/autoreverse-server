-- Extra content for the design comparison (e2e/design/compare.sh), applied after e2e/seed.sql to
-- both the legacy and the Rust database. Plain SQL valid against both schemas, so the two UIs
-- render the same rows: a series, a favorite album and track, a bookmark and a finished book.

UPDATE items SET series = 'E2E-Serie', series_index = 1 WHERE title = 'E2E-Series-Book-A';
UPDATE items SET series = 'E2E-Serie', series_index = 2 WHERE title = 'E2E-Series-Book-B';
UPDATE items SET year = 2021, description = 'Eine Beschreibung, damit die Detailseite Text zeigt.'
WHERE title IN ('E2E-Fixture-Book-EndOfBook', 'E2E-Fixture-Album-Detail');

INSERT INTO favorites (user_id, item_id)
SELECT u.id, i.id FROM users u, items i
WHERE u.name = 'Oliver' AND i.title IN ('E2E-Fixture-Album-Detail', 'E2E-Fixture-Book-EndOfBook')
ON CONFLICT DO NOTHING;

INSERT INTO favorites (user_id, track_id)
SELECT u.id, t.id FROM users u, tracks t JOIN items i ON i.id = t.item_id
WHERE u.name = 'Oliver' AND i.title = 'E2E-Fixture-Album-Detail' AND t.position = 2
ON CONFLICT DO NOTHING;

INSERT INTO bookmarks (user_id, item_id, position, title)
SELECT u.id, i.id, 754, 'Gute Stelle' FROM users u, items i
WHERE u.name = 'Oliver' AND i.title = 'E2E-Fixture-Book-EndOfBook'
ON CONFLICT DO NOTHING;
