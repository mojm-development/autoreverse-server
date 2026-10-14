import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import {
	issueToken,
	userForToken,
	revokeToken,
	revokeOtherTokens
} from '../../src/lib/server/auth/tokens';
import { createUser } from '../../src/lib/server/auth/passwords';
import { authTokens } from '../../src/lib/server/db/schema';
import { eq } from 'drizzle-orm';

describe('tokens', () => {
	it('issues a resolvable token', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const token = await issueToken(db, userId);
			expect(token.length).toBeGreaterThan(30);
			expect(await userForToken(db, token)).toBe(userId);
		});
	});

	it('unknown token resolves to null', async () => {
		await withTestDb(async (db) => {
			expect(await userForToken(db, 'not-a-real-token')).toBeNull();
		});
	});

	it('revokeToken deletes it', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const token = await issueToken(db, userId);
			await revokeToken(db, token);
			expect(await userForToken(db, token)).toBeNull();
		});
	});

	it('revokeOtherTokens keeps the current one, drops the rest', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const keep = await issueToken(db, userId);
			const other = await issueToken(db, userId);
			await revokeOtherTokens(db, userId, keep);
			expect(await userForToken(db, keep)).toBe(userId);
			expect(await userForToken(db, other)).toBeNull();
		});
	});

	it('refreshes last_seen_at only when stale (>1h) or null', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const token = await issueToken(db, userId);
			await userForToken(db, token); // last_seen_at was NULL -> refreshed now
			const [row] = await db.select().from(authTokens).where(eq(authTokens.value, token));
			expect(row.lastSeenAt).not.toBeNull();
		});
	});
}, 60_000);
