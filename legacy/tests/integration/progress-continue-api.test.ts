import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import {
	items as itemsTable,
	tracks as tracksTable,
	progress as progressTable
} from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';
import { callRoute } from './_callRoute';
import { _progressContinueGetHandler } from '../../src/routes/progress/continue/+server';

describe('GET /progress/continue', () => {
	it('returns duration and has_cover alongside the position', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({
					kind: 'book',
					title: 'Eine Billion Dollar',
					sortTitle: 'eine billion dollar',
					coverPath: '/covers/billion.jpg'
				})
				.returning();
			await db.insert(tracksTable).values([
				{ itemId: book.id, position: 1, path: '/a', duration: 100 },
				{ itemId: book.id, position: 2, path: '/b', duration: 200 }
			]);
			await db
				.insert(progressTable)
				.values({ userId, itemId: book.id, position: 50, finished: false });

			const res = await callRoute(_progressContinueGetHandler, {
				db,
				locals: { userId, token: null }
			});
			const body = await res.json();

			expect(body.items).toHaveLength(1);
			// Without `duration` a client cannot draw a progress bar without fetching the
			// whole item — that is the entire reason the field is here.
			expect(body.items[0].duration).toBe(300);
			expect(body.items[0].position).toBe(50);
			expect(body.items[0].has_cover).toBe(true);
		});
	});

	it('reports has_cover false when the item has no cover', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'Ohne Bild', sortTitle: 'ohne bild' })
				.returning();
			await db
				.insert(progressTable)
				.values({ userId, itemId: book.id, position: 5, finished: false });

			const res = await callRoute(_progressContinueGetHandler, {
				db,
				locals: { userId, token: null }
			});
			const body = await res.json();

			expect(body.items[0].has_cover).toBe(false);
			// No tracks at all: the LATERAL join coalesces to 0 rather than null, so a
			// client can divide by it without a special case for "unknown".
			expect(body.items[0].duration).toBe(0);
		});
	});
}, 60_000);
