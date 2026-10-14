import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import {
	hashPassword,
	verifyPassword,
	createUser,
	authenticate
} from '../../src/lib/server/auth/passwords';
import { users } from '../../src/lib/server/db/schema';
import { eq } from 'drizzle-orm';

describe('passwords', () => {
	it('hashes and verifies', async () => {
		const hash = await hashPassword('correct horse battery staple');
		expect(await verifyPassword(hash, 'correct horse battery staple')).toBe(true);
		expect(await verifyPassword(hash, 'wrong')).toBe(false);
	});

	it('promotes the first user to admin regardless of the requested flag', async () => {
		await withTestDb(async (db) => {
			const id = await createUser(db, 'oliver', 'hunter2hunter2', false);
			const [row] = await db.select().from(users).where(eq(users.id, id));
			expect(row.isAdmin).toBe(true);
		});
	});

	it('does not promote a later user', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'oliver', 'hunter2hunter2', false);
			const id2 = await createUser(db, 'mara', 'hunter2hunter2', false);
			const [row] = await db.select().from(users).where(eq(users.id, id2));
			expect(row.isAdmin).toBe(false);
		});
	});

	it('authenticate returns null for unknown user without leaking timing shape', async () => {
		await withTestDb(async (db) => {
			expect(await authenticate(db, 'nobody', 'whatever')).toBeNull();
		});
	});

	it('authenticate returns the user id on correct password', async () => {
		await withTestDb(async (db) => {
			const id = await createUser(db, 'oliver', 'hunter2hunter2', false);
			expect(await authenticate(db, 'oliver', 'hunter2hunter2')).toBe(id);
			expect(await authenticate(db, 'oliver', 'wrong')).toBeNull();
		});
	});
}, 60_000);
