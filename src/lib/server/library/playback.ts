import { randomUUID } from 'node:crypto';
import { and, eq, isNull } from 'drizzle-orm';
import { playbackSessions, progress as progressTable, listenEvents } from '../db/schema';
import type { DrizzleDb } from '../db';

export async function openSession(
	db: DrizzleDb,
	userId: number,
	itemId: number,
	startPosition: number
): Promise<string> {
	const id = randomUUID();
	await db.insert(playbackSessions).values({ id, userId, itemId, startPosition });
	return id;
}

export async function closeSession(
	db: DrizzleDb,
	userId: number,
	sessionId: string,
	position: number
) {
	return db.transaction(async (tx) => {
		const [session] = await tx
			.select()
			.from(playbackSessions)
			.where(
				and(
					eq(playbackSessions.id, sessionId),
					eq(playbackSessions.userId, userId),
					isNull(playbackSessions.closedAt)
				)
			);
		if (!session) return null;
		await tx
			.update(playbackSessions)
			.set({ closedAt: new Date() })
			.where(eq(playbackSessions.id, sessionId));
		await savePosition(tx as unknown as DrizzleDb, userId, session.itemId, position);
		const listened = Math.max(0, position - session.startPosition);
		if (listened > 0)
			await tx.insert(listenEvents).values({ userId, itemId: session.itemId, seconds: listened });
		return session;
	});
}

export async function savePosition(
	db: DrizzleDb,
	userId: number,
	itemId: number,
	position: number,
	finished?: boolean
) {
	await db
		.insert(progressTable)
		.values({ userId, itemId, position, finished: finished ?? false, updatedAt: new Date() })
		.onConflictDoUpdate({
			target: [progressTable.userId, progressTable.itemId],
			set: {
				position,
				updatedAt: new Date(),
				...(finished !== undefined ? { finished } : {})
			}
		});
}
