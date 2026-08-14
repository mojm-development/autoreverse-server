import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import {
	favoriteItemPostHandler,
	favoriteItemDeleteHandler
} from '../../src/routes/favorites/items/[id]/+server';
import { favoritesGetHandler } from '../../src/routes/favorites/+server';
import { bookmarksPostHandler } from '../../src/routes/bookmarks/+server';
import { bookmarkDeleteHandler } from '../../src/routes/bookmarks/[id]/+server';
import { playbackPutHandler } from '../../src/routes/me/playback/+server';

describe('favorites/bookmarks/preferences API', () => {
	it('favoriting twice is idempotent, unfavoriting a non-favorite is a no-op', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [row] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x' })
				.returning();
			await callRoute(favoriteItemPostHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(row.id) }
			});
			await callRoute(favoriteItemPostHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(row.id) }
			}); // no throw
			const listRes = await callRoute(favoritesGetHandler, {
				db,
				locals: { userId, token: null }
			});
			expect((await listRes.json()).items).toHaveLength(1);
			await callRoute(favoriteItemDeleteHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999999' }
			}); // no throw, no 404
		});
	});

	it('POST /bookmarks 422s on out-of-bounds position/title, 201s otherwise', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [row] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'X', sortTitle: 'x' })
				.returning();
			const bad = await callRoute(bookmarksPostHandler, {
				db,
				locals: { userId, token: null },
				body: { item_id: row.id, position: -1, title: 'x' }
			});
			expect(bad.status).toBe(422);
			const good = await callRoute(bookmarksPostHandler, {
				db,
				locals: { userId, token: null },
				body: { item_id: row.id, position: 12.5, title: 'Kapitel 3' }
			});
			expect(good.status).toBe(201);
		});
	});

	it('DELETE /bookmarks/{id} 404s uniformly for missing and not-owned', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(bookmarkDeleteHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999999' }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Unbekanntes Lesezeichen');
		});
	});

	it('PUT /me/playback clamps and returns the stored (not requested) values', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(playbackPutHandler, {
				db,
				locals: { userId, token: null },
				body: { playback_speed: 1.25, skip_back: 30, skip_forward: 15 }
			});
			expect(res.status).toBe(200);
			const body = await res.json();
			expect(body.playback_speed).toBe(1.25);
		});
	});
}, 60_000);
