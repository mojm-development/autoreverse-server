import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { users } from '../../src/lib/server/db/schema';

describe('withTestDb', () => {
	it('provisions a real Postgres schema', async () => {
		await withTestDb(async (db) => {
			const [row] = await db
				.insert(users)
				.values({ name: 'oliver', passwordHash: 'x' })
				.returning();
			expect(row.name).toBe('oliver');
			expect(row.isAdmin).toBe(false);
		});
	}, 60_000);
});
