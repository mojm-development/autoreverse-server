import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { countItems, PAGE_SIZE } from '../../src/lib/server/library/queries';
import { callRoute } from './_callRoute';
import { _libraryMoreGetHandler } from '../../src/routes/library/more/+server';
import type { DrizzleDb } from '../../src/lib/server/db';

async function seedAlbums(db: DrizzleDb, count: number) {
	for (let i = 1; i <= count; i++) {
		const title = `Album ${String(i).padStart(3, '0')}`;
		await db
			.insert(itemsTable)
			.values({ kind: 'album', title, sortTitle: title.toLowerCase(), artist: 'X' });
	}
}

function url(params: Record<string, string>) {
	return `http://test/library/more?${new URLSearchParams(params)}`;
}

describe('library paging endpoint', () => {
	it('returns a page at the given offset, in the same order as the first page', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await seedAlbums(db, 5);

			const first = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', limit: '2', offset: '0' })
			});
			const second = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', limit: '2', offset: '2' })
			});
			const firstTitles = (await first.json()).items.map((i: { title: string }) => i.title);
			const secondTitles = (await second.json()).items.map((i: { title: string }) => i.title);
			expect(firstTitles).toEqual(['Album 001', 'Album 002']);
			expect(secondTitles).toEqual(['Album 003', 'Album 004']);
		});
	});

	it('reports hasMore false on the last, short page so scrolling stops', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await seedAlbums(db, 3);

			const page = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', limit: '2', offset: '2' })
			});
			const body = await page.json();
			expect(body.items).toHaveLength(1);
			expect(body.hasMore).toBe(false);
		});
	});

	it('carries the durations the list view needs', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 1, path: '/a.mp3', duration: 90 });

			const page = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', limit: '10', offset: '0' })
			});
			expect((await page.json()).durations[album.id]).toBe(90);
		});
	});

	it('refuses an unknown kind and requires a session', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const bad = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'podcast' })
			});
			expect(bad.status).toBe(422);

			const anonymous = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId: null, token: null },
				url: url({ kind: 'album' })
			});
			expect(anonymous.status).toBe(401);
		});
	});

	it('caps the page size a caller can ask for', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', limit: String(PAGE_SIZE + 1) })
			});
			expect(res.status).toBe(422);
		});
	});

	it('applies the search term to the page, and countItems agrees with it', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await seedAlbums(db, 3);
			await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'Nordlicht', sortTitle: 'nordlicht' });

			const page = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', q: 'nord', limit: '10', offset: '0' })
			});
			const body = await page.json();
			expect(body.items.map((i: { title: string }) => i.title)).toEqual(['Nordlicht']);
			expect(await countItems(db, 'album', { q: 'nord' })).toBe(1);
			expect(await countItems(db, 'album')).toBe(4);
		});
	});

	it('does not hand out items whose files are gone', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await db.insert(itemsTable).values({
				kind: 'album',
				title: 'Weg',
				sortTitle: 'weg',
				missingSince: new Date('2020-01-01')
			});
			const page = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: url({ kind: 'album', limit: '10', offset: '0' })
			});
			expect((await page.json()).items).toEqual([]);
		});
	});
});
