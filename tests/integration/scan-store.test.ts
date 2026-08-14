import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { storeItems, markMissing, knownFiles } from '../../src/lib/server/scanner/store';
import type { ScannedItem } from '../../src/lib/server/scanner/books';

function fakeScanned(overrides: Partial<ScannedItem> = {}): ScannedItem {
	return {
		sourcePath: '/library/books/A/B',
		kind: 'book',
		title: 'B',
		author: 'A',
		series: null,
		seriesIndex: null,
		tracks: [{ path: '/library/books/A/B/01.mp3', position: 1, title: 'T', disc: null, duration: 10, mtime: 100, size: 200 }],
		chapters: [{ title: 'T', start: 0, end: 10 }],
		unchanged: false,
		...overrides
	};
}

describe('storeItems', () => {
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
			const report = await storeItems(db, [fakeScanned({ title: 'B renamed' })], '/library/books', '/data/covers');
			expect(report.updated).toBe(1);
			const rows = await db.select().from(itemsTable);
			expect(rows).toHaveLength(1);
			expect(rows[0].title).toBe('B renamed');
		});
	});

	it('clears missing_since on an item found again', async () => {
		await withTestDb(async (db) => {
			await db.insert(itemsTable).values({ kind: 'book', title: 'B', sortTitle: 'b', sourcePath: '/library/books/A/B', missingSince: new Date() });
			await storeItems(db, [fakeScanned()], '/library/books', '/data/covers');
			const [row] = await db.select().from(itemsTable).where(eq(itemsTable.sourcePath, '/library/books/A/B'));
			expect(row.missingSince).toBeNull();
		});
	});

	it('removes tracks no longer present in the folder before re-upserting positions', async () => {
		await withTestDb(async (db) => {
			await storeItems(db, [fakeScanned({ tracks: [
				{ path: '/library/books/A/B/01.mp3', position: 1, title: 'T1', disc: null, duration: 5, mtime: 1, size: 1 },
				{ path: '/library/books/A/B/02.mp3', position: 2, title: 'T2', disc: null, duration: 5, mtime: 1, size: 1 }
			] })], '/library/books', '/data/covers');
			await storeItems(db, [fakeScanned({ tracks: [
				{ path: '/library/books/A/B/01.mp3', position: 1, title: 'T1', disc: null, duration: 5, mtime: 1, size: 1 }
			] })], '/library/books', '/data/covers');
			const [item] = await db.select().from(itemsTable);
			const rows = await db.select().from(tracksTable).where(eq(tracksTable.itemId, item.id));
			expect(rows).toHaveLength(1);
		});
	});
});

describe('markMissing', () => {
	it('marks items under root not found this run, path-prefix scoped (root/x does not match root2)', async () => {
		await withTestDb(async (db) => {
			await db.insert(itemsTable).values([
				{ kind: 'book', title: 'A', sortTitle: 'a', sourcePath: '/library/books/A' },
				{ kind: 'book', title: 'B', sortTitle: 'b', sourcePath: '/library/books2/B' } // different root prefix, must NOT be touched
			]);
			const marked = await markMissing(db, '/library/books', new Set(), []);
			expect(marked).toBe(1);
			const rows = await db.select().from(itemsTable);
			const a = rows.find((r) => r.sourcePath === '/library/books/A')!;
			const b = rows.find((r) => r.sourcePath === '/library/books2/B')!;
			expect(a.missingSince).not.toBeNull();
			expect(b.missingSince).toBeNull();
		});
	});

	it('does not re-touch an already-missing item (denominator fix)', async () => {
		await withTestDb(async (db) => {
			const [row] = await db.insert(itemsTable).values({ kind: 'book', title: 'A', sortTitle: 'a', sourcePath: '/library/books/A', missingSince: new Date('2020-01-01') }).returning();
			await markMissing(db, '/library/books', new Set(), []);
			const [after] = await db.select().from(itemsTable).where(eq(itemsTable.id, row.id));
			expect(after.missingSince?.getTime()).toBe(row.missingSince!.getTime()); // unchanged, not re-stamped
		});
	});
}, 60_000);
