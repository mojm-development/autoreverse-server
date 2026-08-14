import { describe, it, expect, beforeEach } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { scanState } from '../../src/lib/server/admin/scanState';
import { callRoute } from './_callRoute';
import { scanPostHandler } from '../../src/routes/scan/+server';
import { scanCancelPostHandler } from '../../src/routes/scan/cancel/+server';
import { scanStatusGetHandler } from '../../src/routes/scan/status/+server';
import { itemsMissingDeleteHandler } from '../../src/routes/items/missing/+server';
import { items as itemsTable } from '../../src/lib/server/db/schema';

describe('admin scan API', () => {
	beforeEach(() => {
		scanState.running = false;
		scanState.cancelRequested = false;
	});

	it('POST /scan 409s while a scan is already running', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			scanState.running = true;
			const res = await callRoute(scanPostHandler, { db, locals: { userId, token: null } });
			expect(res.status).toBe(409);
			expect((await res.json()).detail).toBe('Es läuft bereits ein Scan');
		});
	});

	it('POST /scan/cancel 409s when nothing is running', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(scanCancelPostHandler, { db, locals: { userId, token: null } });
			expect(res.status).toBe(409);
			expect((await res.json()).detail).toBe('Es läuft gerade kein Scan');
		});
	});

	it('GET /scan/status returns the ScanStatus shape', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(scanStatusGetHandler, { db, locals: { userId, token: null } });
			const body = await res.json();
			expect(body).toHaveProperty('running');
			expect(body).toHaveProperty('last_report');
			expect(body).toHaveProperty('progress');
		});
	});

	it('non-admin gets 403 from /scan/status', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'admin', 'hunter2hunter2', true); // first user is always admin — create a throwaway one first
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', false);
			const res = await callRoute(scanStatusGetHandler, { db, locals: { userId, token: null } });
			expect(res.status).toBe(403);
		});
	});

	it('DELETE /items/missing removes missing items and 409s mid-scan', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'X', sortTitle: 'x', missingSince: new Date() });
			const res = await callRoute(itemsMissingDeleteHandler, {
				db,
				locals: { userId, token: null }
			});
			expect(res.status).toBe(200);
			expect((await res.json()).removed).toBe(1);

			scanState.running = true;
			const res2 = await callRoute(itemsMissingDeleteHandler, {
				db,
				locals: { userId, token: null }
			});
			expect(res2.status).toBe(409);
		});
	});
}, 60_000);
