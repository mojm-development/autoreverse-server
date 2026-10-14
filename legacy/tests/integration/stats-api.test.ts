import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable, listenEvents } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _meStatsGetHandler } from '../../src/routes/me/stats/+server';

async function bookId(db: Parameters<typeof createUser>[0]) {
	const [row] = await db
		.insert(itemsTable)
		.values({ kind: 'book', title: 'Der Schwarm', sortTitle: 'der schwarm' })
		.returning({ id: itemsTable.id });
	return row.id;
}

describe('GET /me/stats', () => {
	it('sums listen events per day and over the window', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const itemId = await bookId(db);
			const today = new Date();
			await db.insert(listenEvents).values([
				{ userId, itemId, seconds: 600, at: today },
				{ userId, itemId, seconds: 300, at: today }
			]);

			const res = await callRoute(_meStatsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/me/stats'
			});
			const body = await res.json();

			expect(res.status).toBe(200);
			expect(body.total_seconds).toBe(900);
			const day = today.toISOString().slice(0, 10);
			expect(body.days[day]).toBe(900);
		});
	});

	it('counts only the asking user', async () => {
		await withTestDb(async (db) => {
			const mine = await createUser(db, 'oliver', 'hunter2hunter2');
			const theirs = await createUser(db, 'someone', 'hunter2hunter2');
			const itemId = await bookId(db);
			await db.insert(listenEvents).values([
				{ userId: mine, itemId, seconds: 100 },
				{ userId: theirs, itemId, seconds: 9999 }
			]);

			const res = await callRoute(_meStatsGetHandler, {
				db,
				locals: { userId: mine, token: null },
				url: 'http://test/me/stats'
			});

			expect((await res.json()).total_seconds).toBe(100);
		});
	});

	it('leaves out anything older than the requested window', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const itemId = await bookId(db);
			const longAgo = new Date(Date.now() - 60 * 24 * 3600 * 1000);
			await db.insert(listenEvents).values([
				{ userId, itemId, seconds: 500, at: longAgo },
				{ userId, itemId, seconds: 42 }
			]);

			const res = await callRoute(_meStatsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/me/stats?days=7'
			});
			const body = await res.json();

			expect(body.total_seconds).toBe(42);
			// A day without listening is absent rather than zero — the client draws a
			// bar chart, where a missing bar and a zero bar mean the same thing.
			expect(Object.keys(body.days)).toHaveLength(1);
		});
	});

	it('reports nothing rather than failing for a user who never listened', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_meStatsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/me/stats'
			});
			const body = await res.json();
			expect(body).toEqual({ total_seconds: 0, days: {} });
		});
	});

	it('rejects an anonymous caller', async () => {
		await withTestDb(async (db) => {
			const res = await callRoute(_meStatsGetHandler, {
				db,
				locals: { userId: null, token: null },
				url: 'http://test/me/stats'
			});
			expect(res.status).toBe(401);
		});
	});
});
