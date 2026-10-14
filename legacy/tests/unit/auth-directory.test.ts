import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { listUsers, setAdmin, LastAdminError } from '../../src/lib/server/auth/directory';

describe('directory', () => {
	it('lists users sorted case-insensitively by name, no password hash', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'zoe', 'hunter2hunter2');
			await createUser(db, 'Anna', 'hunter2hunter2');
			const rows = await listUsers(db);
			expect(rows.map((r) => r.name)).toEqual(['Anna', 'zoe']);
			expect(rows[0]).not.toHaveProperty('passwordHash');
		});
	});

	it('refuses to demote the last admin', async () => {
		await withTestDb(async (db) => {
			const id = await createUser(db, 'oliver', 'hunter2hunter2'); // auto-admin (first user)
			await expect(setAdmin(db, id, false)).rejects.toBeInstanceOf(LastAdminError);
		});
	});

	it('allows demotion when another admin exists', async () => {
		await withTestDb(async (db) => {
			const first = await createUser(db, 'oliver', 'hunter2hunter2');
			const second = await createUser(db, 'mara', 'hunter2hunter2');
			await setAdmin(db, second, true);
			await expect(setAdmin(db, first, false)).resolves.toBe(false);
		});
	});
}, 60_000);
