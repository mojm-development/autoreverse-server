import { randomBytes } from 'node:crypto';
import { and, eq, ne, sql } from 'drizzle-orm';
import { authTokens } from '../db/schema';
import type { DrizzleDb } from '../db';

export async function issueToken(db: DrizzleDb, userId: number): Promise<string> {
	const value = randomBytes(32).toString('base64url');
	await db.insert(authTokens).values({ value, userId });
	return value;
}

export async function userForToken(db: DrizzleDb, token: string): Promise<number | null> {
	const [row] = await db
		.select({
			userId: authTokens.userId,
			stale: sql<boolean>`(${authTokens.lastSeenAt} IS NULL OR ${authTokens.lastSeenAt} < now() - interval '1 hour')`
		})
		.from(authTokens)
		.where(eq(authTokens.value, token));
	if (!row) return null;
	if (row.stale) {
		await db
			.update(authTokens)
			.set({ lastSeenAt: sql`now()` })
			.where(eq(authTokens.value, token));
	}
	return row.userId;
}

export async function revokeToken(db: DrizzleDb, token: string): Promise<void> {
	await db.delete(authTokens).where(eq(authTokens.value, token));
}

export async function revokeOtherTokens(
	db: DrizzleDb,
	userId: number,
	keep: string
): Promise<void> {
	await db.delete(authTokens).where(and(eq(authTokens.userId, userId), ne(authTokens.value, keep)));
}
