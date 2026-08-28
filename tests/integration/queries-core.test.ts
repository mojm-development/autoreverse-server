import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import {
	items,
	tracks,
	countItems,
	countMissing,
	deleteMissing,
	artists,
	albumsOfArtist,
	searchItems,
	libraryCounts
} from '../../src/lib/server/library/queries';
import type { DrizzleDb } from '../../src/lib/server/db';

async function seedAlbum(db: DrizzleDb, overrides: Partial<typeof itemsTable.$inferInsert> = {}) {
	const [row] = await db
		.insert(itemsTable)
		.values({
			kind: 'album',
			title: 'Nordlicht',
			sortTitle: 'nordlicht',
			artist: 'Ansa Volt',
			...overrides
		})
		.returning();
	return row;
}

describe('core item queries', () => {
	it('items() filters by kind, is case-insensitive on q, and paginates', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, { title: 'Nordlicht', sortTitle: 'nordlicht' });
			await seedAlbum(db, { title: 'Halbschatten', sortTitle: 'halbschatten' });
			await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'Eine Billion Dollar', sortTitle: 'eine billion dollar' });

			const albums = await items(db, { kind: 'album', limit: 10, offset: 0, sort: 'title' });
			expect(albums).toHaveLength(2);
			expect(albums[0].title).toBe('Halbschatten'); // alphabetical by sort_title

			const hits = await items(db, { q: 'BILLION', limit: 10, offset: 0, sort: 'title' });
			expect(hits.map((i) => i.title)).toEqual(['Eine Billion Dollar']);
		});
	});

	it('items() excludes children (parent_id set)', async () => {
		await withTestDb(async (db) => {
			const podcast = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'Maschinenraum', sortTitle: 'maschinenraum' })
				.returning();
			await db.insert(itemsTable).values({
				kind: 'episode',
				parentId: podcast[0].id,
				title: 'Folge 1',
				sortTitle: 'folge 1'
			});
			const top = await items(db, { limit: 10, offset: 0, sort: 'title' });
			expect(top).toHaveLength(1);
		});
	});

	it('missing=true / false filter on missing_since', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, { missingSince: new Date() });
			await seedAlbum(db, { title: 'Present', sortTitle: 'present' });
			expect(await items(db, { missing: true, limit: 10, offset: 0, sort: 'title' })).toHaveLength(
				1
			);
			expect(await items(db, { missing: false, limit: 10, offset: 0, sort: 'title' })).toHaveLength(
				1
			);
		});
	});

	it('hides items that vanished from disk from every album listing', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, { title: 'Weg', sortTitle: 'weg', missingSince: new Date() });
			await seedAlbum(db, { title: 'Da', sortTitle: 'da' });

			expect(
				(await items(db, { kind: 'album', missing: false, limit: 10, offset: 0 })).length
			).toBe(1);
			expect(await countItems(db, 'album')).toBe(1);
			expect((await libraryCounts(db)).album_count).toBe(1);
			expect(await artists(db)).toEqual([{ name: 'Ansa Volt', albumCount: 1 }]);
			expect((await albumsOfArtist(db, 'Ansa Volt')).map((a) => a.title)).toEqual(['Da']);
			expect(await searchItems(db, 'Weg', ['album'], 10)).toHaveLength(0);
			expect(await countMissing(db, 'album')).toBe(1);
		});
	});

	it('drops an artist whose every album is gone', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, { missingSince: new Date() });
			expect(await artists(db)).toHaveLength(0);
		});
	});

	it('tracks()/chapters() return in position order', async () => {
		await withTestDb(async (db) => {
			const album = await seedAlbum(db);
			await db.insert(tracksTable).values([
				{ itemId: album.id, position: 2, path: '/b', duration: 10 },
				{ itemId: album.id, position: 1, path: '/a', duration: 5 }
			]);
			const rows = await tracks(db, album.id);
			expect(rows.map((r) => r.position)).toEqual([1, 2]);
		});
	});

	it('deleteMissing() removes only missing_since IS NOT NULL rows', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, { missingSince: new Date() });
			await seedAlbum(db, { title: 'Present', sortTitle: 'present' });
			const removed = await deleteMissing(db);
			expect(removed).toBe(1);
			expect(await countItems(db, 'album')).toBe(1);
		});
	});
}, 60_000);
