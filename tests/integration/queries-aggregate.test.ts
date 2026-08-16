import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import {
	items as itemsTable,
	tracks as tracksTable,
	progress as progressTable,
	playlists as playlistsTable,
	playlistEntries
} from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';
import {
	continueListening,
	progressMap,
	itemDurations,
	searchArtists,
	playlistOverview
} from '../../src/lib/server/library/queries';

describe('aggregate queries', () => {
	it('continueListening sums track durations via LATERAL join and orders by updated_at desc', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({
					kind: 'book',
					title: 'Eine Billion Dollar',
					sortTitle: 'eine billion dollar',
					author: 'Andreas Eschbach'
				})
				.returning();
			await db.insert(tracksTable).values([
				{ itemId: book.id, position: 1, path: '/a', duration: 100 },
				{ itemId: book.id, position: 2, path: '/b', duration: 200 }
			]);
			await db
				.insert(progressTable)
				.values({ userId, itemId: book.id, position: 50, finished: false });

			const rows = await continueListening(db, userId);
			expect(rows).toHaveLength(1);
			expect(rows[0].duration).toBe(300);
			expect(rows[0].position).toBe(50);
		});
	});

	it('continueListening excludes finished items', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'X', sortTitle: 'x' })
				.returning();
			await db
				.insert(progressTable)
				.values({ userId, itemId: book.id, position: 999, finished: true });
			expect(await continueListening(db, userId)).toHaveLength(0);
		});
	});

	it('progressMap batches lookups for multiple items in one call', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [a] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'A', sortTitle: 'a' })
				.returning();
			const [b] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'B', sortTitle: 'b' })
				.returning();
			await db.insert(progressTable).values([
				{ userId, itemId: a.id, position: 10 },
				{ userId, itemId: b.id, position: 20 }
			]);
			const map = await progressMap(db, userId, [a.id, b.id]);
			expect(map[a.id].position).toBe(10);
			expect(map[b.id].position).toBe(20);
		});
	});

	it('progressMap returns {} for an empty id list without querying', async () => {
		await withTestDb(async (db) => {
			expect(await progressMap(db, 1, [])).toEqual({});
		});
	});

	it('itemDurations sums track durations per item and omits items without tracks', async () => {
		await withTestDb(async (db) => {
			const [withTracks] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			const [withoutTracks] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'B', sortTitle: 'b' })
				.returning();
			await db.insert(tracksTable).values([
				{ itemId: withTracks.id, position: 1, path: '/a', duration: 100 },
				{ itemId: withTracks.id, position: 2, path: '/b', duration: 50 }
			]);
			const map = await itemDurations(db, [withTracks.id, withoutTracks.id]);
			expect(map[withTracks.id]).toBe(150);
			expect(withoutTracks.id in map).toBe(false);
		});
	});

	it('itemDurations returns {} for an empty id list without querying', async () => {
		await withTestDb(async (db) => {
			expect(await itemDurations(db, [])).toEqual({});
		});
	});

	it('searchArtists groups album artists and book authors separately', async () => {
		await withTestDb(async (db) => {
			await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x', artist: 'Ansa Volt' });
			await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'Y', sortTitle: 'y', author: 'Ansa Volt' });
			const rows = await searchArtists(db, 'ansa', 20);
			expect(rows.map((r: { role: string }) => r.role).sort()).toEqual(['artist', 'author']);
		});
	});

	it('playlistOverview sums both single-track and whole-item entry durations', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [playlist] = await db
				.insert(playlistsTable)
				.values({ userId, name: 'Fahrtwind' })
				.returning();
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x' })
				.returning();
			const [track1] = await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 1, path: '/a', duration: 100 })
				.returning();
			await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 2, path: '/b', duration: 50 })
				.returning();
			await db.insert(playlistEntries).values([
				{ playlistId: playlist.id, trackId: track1.id, position: 1 }, // single track: 100
				{ playlistId: playlist.id, itemId: album.id, position: 2 } // whole item: 100 + 50
			]);
			const [overview] = await playlistOverview(db, userId);
			expect(overview.entryCount).toBe(2);
			expect(overview.duration).toBe(250);
		});
	});
}, 60_000);
