import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { callRoute } from './_callRoute';
import { serverInfoGetHandler } from '../../src/routes/server/info/+server';

describe('GET /server/info', () => {
	it('includes user_count for an admin', async () => {
		await withTestDb(async (db) => {
			const admin = await createUser(db, 'oliver', 'hunter2hunter2'); // first user, auto-admin
			const res = await callRoute(serverInfoGetHandler, {
				db,
				locals: { userId: admin, token: null }
			});
			expect((await res.json()).user_count).toBe(1);
		});
	});

	it('omits (null) user_count for a non-admin', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'oliver', 'hunter2hunter2');
			const nonAdmin = await createUser(db, 'mara', 'hunter2hunter2');
			const res = await callRoute(serverInfoGetHandler, {
				db,
				locals: { userId: nonAdmin, token: null }
			});
			expect((await res.json()).user_count).toBeNull();
		});
	});
}, 60_000);
