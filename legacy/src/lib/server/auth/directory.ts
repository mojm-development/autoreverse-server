import { eq, sql } from 'drizzle-orm';
import { users, authTokens } from '../db/schema';
import type { DrizzleDb } from '../db';

export class LastAdminError extends Error {}

export async function listUsers(db: DrizzleDb) {
	const rows = await db
		.select({
			id: users.id,
			name: users.name,
			isAdmin: users.isAdmin,
			createdAt: users.createdAt,
			lastSeenAt: sql<Date | null>`max(${authTokens.lastSeenAt})`
		})
		.from(users)
		.leftJoin(authTokens, eq(authTokens.userId, users.id))
		.groupBy(users.id)
		.orderBy(sql`lower(${users.name})`);
	return rows;
}

export async function setAdmin(db: DrizzleDb, userId: number, isAdmin: boolean): Promise<boolean> {
	return db.transaction(async (tx) => {
		if (!isAdmin) {
			const admins = await tx.execute(sql`SELECT id FROM users WHERE is_admin FOR UPDATE`);
			const adminIds = (admins as unknown as Array<{ id: number }>).map((r) => r.id);
			if (adminIds.length <= 1 && adminIds.includes(userId)) throw new LastAdminError();
		}
		const [row] = await tx
			.update(users)
			.set({ isAdmin })
			.where(eq(users.id, userId))
			.returning({ isAdmin: users.isAdmin });
		if (!row) throw new Error(`user ${userId} not found`);
		return row.isAdmin;
	});
}
