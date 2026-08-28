import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import {
	items as itemsTable,
	tracks as tracksTable,
	progress as progressTable,
	bookmarks as bookmarksTable
} from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';
import { eq } from 'drizzle-orm';
import { storeItems, removeVanished } from '../../src/lib/server/scanner/store';
import type { ScannedItem, ScanFailure } from '../../src/lib/server/scanner/books';

function fakeScanned(overrides: Partial<ScannedItem> = {}): ScannedItem {
	return {
		sourcePath: '/library/books/A/B',
		kind: 'book',
		title: 'B',
		author: 'A',
		series: null,
		seriesIndex: null,
		tracks: [
			{
				path: '/library/books/A/B/01.mp3',
				position: 1,
				title: 'T',
				disc: null,
				duration: 10,
				mtime: 100,
				size: 200
			}
		],
		chapters: [{ title: 'T', start: 0, end: 10 }],
		unchanged: false,
		...overrides
	};
}

describe('storeItems', () => {
	it('skips a folder whose insert fails and stores the rest', async () => {
		await withTestDb(async (db) => {
			// Two tracks claiming the same position collide on
			// UNIQUE(item_id, position). scanFolder no longer produces this, but
			// storeItems must survive whatever it is handed — before, the throw
			// escaped the loop and every later folder was abandoned with it.
			const broken = fakeScanned({
				sourcePath: '/library/books/A/Kaputt',
				title: 'Kaputt',
				tracks: [
					{
						path: '/library/books/A/Kaputt/01.mp3',
						position: 1,
						title: 'eins',
						disc: null,
						duration: 10,
						mtime: 100,
						size: 200
					},
					{
						path: '/library/books/A/Kaputt/02.mp3',
						position: 1,
						title: 'zwei',
						disc: null,
						duration: 10,
						mtime: 100,
						size: 200
					}
				]
			});
			const healthy = fakeScanned({ sourcePath: '/library/books/A/Heil', title: 'Heil' });

			const failures: ScanFailure[] = [];
			const report = await storeItems(
				db,
				[broken, healthy],
				'/library/books',
				'/data/covers',
				failures
			);

			expect(report.skipped).toBe(1);
			expect(report.new).toBe(1);
			expect(failures.map((f) => f.path)).toEqual(['/library/books/A/Kaputt']);
			const stored = await db.select().from(itemsTable);
			expect(stored.map((i) => i.title)).toEqual(['Heil']);
		});
	});

	it('spares a failed folder from being marked missing', async () => {
		await withTestDb(async (db) => {
			await storeItems(db, [fakeScanned()], '/library/books', '/data/covers');
			// Not in `found` because it failed — but it was never observed absent,
			// so passing it as skipped must keep missing_since null.
			const removed = await removeVanished(db, '/library/books', new Set(), ['/library/books/A/B']);
			expect(removed).toBe(0);
			const rows = await db.select().from(itemsTable);
			expect(rows).toHaveLength(1);
		});
	});

	it('inserts a new item with its tracks and chapters', async () => {
		await withTestDb(async (db) => {
			const report = await storeItems(db, [fakeScanned()], '/library/books', '/data/covers');
			expect(report.new).toBe(1);
			const rows = await db.select().from(itemsTable);
			expect(rows).toHaveLength(1);
			expect(rows[0].missingSince).toBeNull();
		});
	});

	it('re-running with the same sourcePath updates, does not duplicate', async () => {
		await withTestDb(async (db) => {
			await storeItems(db, [fakeScanned()], '/library/books', '/data/covers');
			const report = await storeItems(
				db,
				[fakeScanned({ title: 'B renamed' })],
				'/library/books',
				'/data/covers'
			);
			expect(report.updated).toBe(1);
			const rows = await db.select().from(itemsTable);
			expect(rows).toHaveLength(1);
			expect(rows[0].title).toBe('B renamed');
		});
	});

	it('clears missing_since on an item found again', async () => {
		await withTestDb(async (db) => {
			await db.insert(itemsTable).values({
				kind: 'book',
				title: 'B',
				sortTitle: 'b',
				sourcePath: '/library/books/A/B',
				missingSince: new Date()
			});
			await storeItems(db, [fakeScanned()], '/library/books', '/data/covers');
			const [row] = await db
				.select()
				.from(itemsTable)
				.where(eq(itemsTable.sourcePath, '/library/books/A/B'));
			expect(row.missingSince).toBeNull();
		});
	});

	it('removes tracks no longer present in the folder before re-upserting positions', async () => {
		await withTestDb(async (db) => {
			await storeItems(
				db,
				[
					fakeScanned({
						tracks: [
							{
								path: '/library/books/A/B/01.mp3',
								position: 1,
								title: 'T1',
								disc: null,
								duration: 5,
								mtime: 1,
								size: 1
							},
							{
								path: '/library/books/A/B/02.mp3',
								position: 2,
								title: 'T2',
								disc: null,
								duration: 5,
								mtime: 1,
								size: 1
							}
						]
					})
				],
				'/library/books',
				'/data/covers'
			);
			await storeItems(
				db,
				[
					fakeScanned({
						tracks: [
							{
								path: '/library/books/A/B/01.mp3',
								position: 1,
								title: 'T1',
								disc: null,
								duration: 5,
								mtime: 1,
								size: 1
							}
						]
					})
				],
				'/library/books',
				'/data/covers'
			);
			const [item] = await db.select().from(itemsTable);
			const rows = await db.select().from(tracksTable).where(eq(tracksTable.itemId, item.id));
			expect(rows).toHaveLength(1);
		});
	});

	it('swaps two surviving tracks positions on rescan without throwing a UNIQUE violation', async () => {
		await withTestDb(async (db) => {
			await storeItems(
				db,
				[
					fakeScanned({
						tracks: [
							{
								path: '/library/books/A/B/01.mp3',
								position: 1,
								title: 'T1',
								disc: null,
								duration: 5,
								mtime: 1,
								size: 1
							},
							{
								path: '/library/books/A/B/02.mp3',
								position: 2,
								title: 'T2',
								disc: null,
								duration: 5,
								mtime: 1,
								size: 1
							}
						]
					})
				],
				'/library/books',
				'/data/covers'
			);

			// Same two paths, positions swapped -- both tracks still exist, so
			// the delete-tracks-no-longer-present step removes nothing.
			await expect(
				storeItems(
					db,
					[
						fakeScanned({
							tracks: [
								{
									path: '/library/books/A/B/02.mp3',
									position: 1,
									title: 'T2',
									disc: null,
									duration: 5,
									mtime: 1,
									size: 1
								},
								{
									path: '/library/books/A/B/01.mp3',
									position: 2,
									title: 'T1',
									disc: null,
									duration: 5,
									mtime: 1,
									size: 1
								}
							]
						})
					],
					'/library/books',
					'/data/covers'
				)
			).resolves.not.toThrow();

			const [item] = await db.select().from(itemsTable);
			const rows = await db.select().from(tracksTable).where(eq(tracksTable.itemId, item.id));
			expect(rows).toHaveLength(2);
			const track01 = rows.find((r) => r.path === '/library/books/A/B/01.mp3')!;
			const track02 = rows.find((r) => r.path === '/library/books/A/B/02.mp3')!;
			expect(track01.position).toBe(2);
			expect(track02.position).toBe(1);
		});
	});
});

describe('removeVanished', () => {
	it('deletes items under root not found this run, path-prefix scoped (root/x does not match root2)', async () => {
		await withTestDb(async (db) => {
			await db.insert(itemsTable).values([
				{ kind: 'book', title: 'A', sortTitle: 'a', sourcePath: '/library/books/A' },
				{ kind: 'book', title: 'B', sortTitle: 'b', sourcePath: '/library/books2/B' }
			]);
			const removed = await removeVanished(db, '/library/books', new Set(), []);
			expect(removed).toBe(1);
			const rows = await db.select().from(itemsTable);
			expect(rows.map((r) => r.sourcePath)).toEqual(['/library/books2/B']);
		});
	});

	it('keeps an item that was found on disk this run', async () => {
		await withTestDb(async (db) => {
			await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'A', sortTitle: 'a', sourcePath: '/library/books/A' });
			const removed = await removeVanished(db, '/library/books', new Set(['/library/books/A']), []);
			expect(removed).toBe(0);
			expect(await db.select().from(itemsTable)).toHaveLength(1);
		});
	});

	it('deletes the root folder item itself when it is gone', async () => {
		await withTestDb(async (db) => {
			await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'Root', sortTitle: 'root', sourcePath: '/library/books' });
			expect(await removeVanished(db, '/library/books', new Set(), [])).toBe(1);
			expect(await db.select().from(itemsTable)).toHaveLength(0);
		});
	});

	it('clears out rows an older version had only marked missing', async () => {
		await withTestDb(async (db) => {
			await db.insert(itemsTable).values({
				kind: 'book',
				title: 'A',
				sortTitle: 'a',
				sourcePath: '/library/books/A',
				missingSince: new Date('2020-01-01')
			});
			expect(await removeVanished(db, '/library/books', new Set(), [])).toBe(1);
			expect(await db.select().from(itemsTable)).toHaveLength(0);
		});
	});

	it('takes the item listening history with it', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [item] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'A', sortTitle: 'a', sourcePath: '/library/books/A' })
				.returning();
			await db
				.insert(progressTable)
				.values({ userId, itemId: item.id, position: 42, finished: false });
			await db
				.insert(bookmarksTable)
				.values({ userId, itemId: item.id, position: 42, title: 'Stelle' });

			await removeVanished(db, '/library/books', new Set(), []);
			expect(await db.select().from(progressTable)).toHaveLength(0);
			expect(await db.select().from(bookmarksTable)).toHaveLength(0);
		});
	});
}, 60_000);
