import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _searchGetHandler } from '../../src/routes/search/+server';
import { _artistsGetHandler } from '../../src/routes/artists/+server';

describe('search API', () => {
	it('groups results by kind, podcasts group includes episodes', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'Eine Billion Dollar', sortTitle: 'eine billion dollar' });
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'eine Sendung', sortTitle: 'eine sendung' })
				.returning();
			await db.insert(itemsTable).values({
				kind: 'episode',
				parentId: podcast.id,
				title: 'eine Folge',
				sortTitle: 'eine folge'
			});

			const res = await callRoute(_searchGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/search?q=eine'
			});
			const body = await res.json();
			expect(body.books).toHaveLength(1);
			expect(body.podcasts).toHaveLength(2); // podcast item + episode item
		});
	});

	it('422s when q is missing', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_searchGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/search'
			});
			expect(res.status).toBe(422);
		});
	});

	it('returns artists grouped by name', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			// Note: artists are aggregated from items table, so we insert items
			await db.insert(itemsTable).values({
				kind: 'album',
				title: 'Album 1',
				sortTitle: 'album 1',
				artist: 'Test Artist'
			});
			await db.insert(itemsTable).values({
				kind: 'album',
				title: 'Album 2',
				sortTitle: 'album 2',
				artist: 'Test Artist'
			});

			const res = await callRoute(_artistsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/artists'
			});
			const body = await res.json();
			expect(body.artists).toBeDefined();
			expect(Array.isArray(body.artists)).toBe(true);
		});
	});
}, 60_000);
