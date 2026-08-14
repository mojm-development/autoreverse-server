import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
// Route handlers are tested by importing and invoking them directly with a
// constructed RequestEvent-shaped object, avoiding a full HTTP server in unit
// scope — see tests/integration/_callRoute.ts helper introduced here.
import { callRoute } from './_callRoute';
import { POST as usersPost } from '../../src/routes/users/+server';
import { PATCH as userPatch } from '../../src/routes/users/[id]/+server';

describe('users API', () => {
	it('POST /users promotes the first user to admin, second stays a regular user', async () => {
		await withTestDb(async (db) => {
			const first = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(usersPost, {
				db,
				locals: { userId: first, token: null },
				body: { name: 'mara', password: 'hunter2hunter2' }
			});
			expect(res.status).toBe(201);
			const created = await res.json();
			expect(created.is_admin).toBe(false);
		});
	});

	it('POST /users returns 409 on duplicate name', async () => {
		await withTestDb(async (db) => {
			const first = await createUser(db, 'oliver', 'hunter2hunter2');
			await callRoute(usersPost, {
				db,
				locals: { userId: first, token: null },
				body: { name: 'mara', password: 'hunter2hunter2' }
			});
			const res = await callRoute(usersPost, {
				db,
				locals: { userId: first, token: null },
				body: { name: 'mara', password: 'hunter2hunter2' }
			});
			expect(res.status).toBe(409);
			expect((await res.json()).detail).toBe('Nutzername ist bereits vergeben');
		});
	});

	it('PATCH /users/{id} refuses to demote the last admin with 409', async () => {
		await withTestDb(async (db) => {
			const first = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(userPatch, {
				db,
				locals: { userId: first, token: null },
				params: { id: String(first) },
				body: { is_admin: false }
			});
			expect(res.status).toBe(409);
			expect((await res.json()).detail).toBe('Der letzte Verwalter kann nicht herabgestuft werden');
		});
	});
}, 60_000);
