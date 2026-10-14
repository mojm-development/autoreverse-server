import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _itemsGetHandler } from '../../src/routes/items/+server';
import { _itemGetHandler } from '../../src/routes/items/[id]/+server';

describe('items API', () => {
	it('GET /items?sort=added returns the newest first', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			// `addedAt` decides the order, the title deliberately contradicts it — otherwise
			// the test would pass just as well against the old hardcoded title sort.
			await db.insert(itemsTable).values([
				{
					kind: 'album',
					title: 'Alpha',
					sortTitle: 'alpha',
					addedAt: new Date('2020-01-01T00:00:00Z')
				},
				{
					kind: 'album',
					title: 'Zeta',
					sortTitle: 'zeta',
					addedAt: new Date('2026-01-01T00:00:00Z')
				}
			]);

			const res = await callRoute(_itemsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/?kind=album&sort=added'
			});
			const titles = (await res.json()).items.map((i: { title: string }) => i.title);
			expect(titles).toEqual(['Zeta', 'Alpha']);
		});
	});

	it('GET /items refuses an unknown sort instead of quietly using the title', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_itemsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/?sort=nonsense'
			});
			expect(res.status).toBe(422);
			expect((await res.json()).detail).toBe('Unbekannte Sortierung');
		});
	});

	it('GET /items omits cover_path but exposes has_cover', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x', coverPath: '/covers/1.jpg' });
			const res = await callRoute(_itemsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/items'
			});
			const body = await res.json();
			expect(body.items[0].has_cover).toBe(true);
			expect(body.items[0]).not.toHaveProperty('cover_path');
		});
	});

	it('GET /items/{id} 404s with the exact German message', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_itemGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999' }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Unbekanntes Item');
		});
	});

	it('GET /items/{id} includes tracks and chapters', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x' })
				.returning();
			await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 1, path: '/a', duration: 42 });
			const res = await callRoute(_itemGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(album.id) }
			});
			const body = await res.json();
			expect(body.tracks).toHaveLength(1);
			expect(body.tracks[0].duration).toBe(42);
			expect(body.chapters).toEqual([]);
		});
	});
}, 60_000);
