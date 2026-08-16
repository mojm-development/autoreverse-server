import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _itemsGetHandler } from '../../src/routes/items/+server';
import { _itemGetHandler } from '../../src/routes/items/[id]/+server';

describe('items API', () => {
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
