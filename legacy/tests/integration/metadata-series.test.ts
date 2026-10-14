import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';
import { setLibraryPaths } from '../../src/lib/server/settings/libraryPaths';
import { planSeries, undoBatch } from '../../src/lib/server/library/bulkMetadata';

async function seedBooks(db: Parameters<typeof planSeries>[0]) {
	return db
		.insert(itemsTable)
		.values([
			{
				kind: 'book',
				title: 'Die Gefährten',
				sortTitle: 'die gefährten',
				author: 'Tolkien',
				sourcePath: '/books/Tolkien/Der Herr der Ringe 01 - Die Gefährten'
			},
			{
				kind: 'book',
				title: 'Der Fluch',
				sortTitle: 'der fluch',
				author: 'Eschbach',
				sourcePath: '/books/Eschbach/Die drei Sonnen/03 - Der Fluch'
			},
			{
				kind: 'book',
				title: 'Das Parfum',
				sortTitle: 'das parfum',
				author: 'Süskind',
				sourcePath: '/books/Süskind/Das Parfum'
			}
		])
		.returning();
}

describe('series assistant', () => {
	it('detects series and volume from the folder, and leaves a standalone book alone', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await setLibraryPaths(db, { booksDir: '/books', musicDir: '/music' });
			await seedBooks(db);

			const preview = await planSeries(db, userId, { filter: { kind: 'book' } });
			expect(preview.matched).toBe(3);
			// Only the two that actually belong to a series.
			expect(preview.changed).toBe(2);
			expect(preview.batch_id).toBeNull();

			const applied = await planSeries(db, userId, {
				filter: { kind: 'book' },
				dry_run: false
			});
			expect(applied.changed).toBe(2);

			const rows = await db.select().from(itemsTable).orderBy(itemsTable.sortTitle);
			const ringe = rows.find((r) => r.title === 'Die Gefährten')!;
			expect(ringe.series).toBe('Der Herr der Ringe');
			expect(ringe.seriesIndex).toBe(1);
			// Series folder above, volume number in the book's own folder.
			const fluch = rows.find((r) => r.title === 'Der Fluch')!;
			expect(fluch.series).toBe('Die drei Sonnen');
			expect(fluch.seriesIndex).toBe(3);
			// A book that is not in a series stays untouched and unlocked.
			const parfum = rows.find((r) => r.title === 'Das Parfum')!;
			expect(parfum.series).toBeNull();
			expect(parfum.lockedFields).toEqual([]);
		});
	}, 60_000);

	it('assigns one series and numbers it through, and undo takes it back', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await setLibraryPaths(db, { booksDir: '/books', musicDir: '/music' });
			const books = await seedBooks(db);

			const applied = await planSeries(db, userId, {
				ids: books.map((b) => b.id),
				mode: 'assign',
				series: 'Sammlung',
				start: 5,
				dry_run: false
			});
			expect(applied.changed).toBe(3);

			const rows = await db.select().from(itemsTable).orderBy(itemsTable.sortTitle);
			// Numbered in the order the list shows: sorted by title, starting at five.
			expect(rows.map((r) => [r.title, r.seriesIndex])).toEqual([
				['Das Parfum', 5],
				['Der Fluch', 6],
				['Die Gefährten', 7]
			]);
			expect(rows.every((r) => r.series === 'Sammlung')).toBe(true);
			expect(rows[0].lockedFields).toEqual(['series', 'seriesIndex']);

			const undone = await undoBatch(db, applied.batch_id!);
			expect(undone.skipped).toBe(0);
			const after = await db.select().from(itemsTable);
			expect(after.every((r) => r.series === null && r.seriesIndex === null)).toBe(true);
			expect(after.every((r) => r.lockedFields.length === 0)).toBe(true);
		});
	}, 60_000);

	it('refuses an assignment without a name and a selection without books', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const books = await seedBooks(db);
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'Nevermind', sortTitle: 'nevermind' })
				.returning();

			await expect(
				planSeries(db, userId, { ids: books.map((b) => b.id), mode: 'assign' })
			).rejects.toThrow('Serienname fehlt');
			// Albums have no series; a selection of them has nothing to plan.
			await expect(planSeries(db, userId, { ids: [album.id] })).rejects.toThrow(
				'Keine Hörbücher ausgewählt'
			);
		});
	}, 60_000);
}, 60_000);
