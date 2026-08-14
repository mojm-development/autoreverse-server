import { and, eq, gt, gte, lt, lte, sql } from 'drizzle-orm';
import { playlistEntries } from '../db/schema';
import type { DrizzleDb } from '../db';

/** Locks the parent playlist row to serialize concurrent mutations —
 * every append/remove/move does this first, inside its own transaction. */
async function lockPlaylist(tx: DrizzleDb, playlistId: number) {
	await tx.execute(sql`SELECT id FROM playlists WHERE id = ${playlistId} FOR UPDATE`);
}

export async function appendEntry(
	db: DrizzleDb,
	playlistId: number,
	entry: { itemId?: number; trackId?: number }
): Promise<number> {
	return db.transaction(async (tx) => {
		await lockPlaylist(tx as unknown as DrizzleDb, playlistId);
		const [{ next }] = await tx
			.select({ next: sql<number>`coalesce(max(${playlistEntries.position}), 0) + 1` })
			.from(playlistEntries)
			.where(eq(playlistEntries.playlistId, playlistId));
		const [row] = await tx
			.insert(playlistEntries)
			.values({
				playlistId,
				itemId: entry.itemId ?? null,
				trackId: entry.trackId ?? null,
				position: next
			})
			.returning({ id: playlistEntries.id });
		return row.id;
	});
}

export async function removeEntry(
	db: DrizzleDb,
	playlistId: number,
	entryId: number,
	position: number
): Promise<void> {
	await db.transaction(async (tx) => {
		await lockPlaylist(tx as unknown as DrizzleDb, playlistId);
		await tx.delete(playlistEntries).where(eq(playlistEntries.id, entryId));
		await tx
			.update(playlistEntries)
			.set({ position: sql`${playlistEntries.position} - 1` })
			.where(
				and(eq(playlistEntries.playlistId, playlistId), gt(playlistEntries.position, position))
			);
	});
}

/** Exact port of library/playlists.py::move_entry — stage the moved row at a
 * sentinel position (-1, outside the valid 1..n range) before shifting the
 * range between old and new position, so no intermediate UPDATE can collide
 * with UNIQUE (playlist_id, position). */
export async function moveEntry(
	db: DrizzleDb,
	playlistId: number,
	entryId: number,
	oldPosition: number,
	newPosition: number
): Promise<void> {
	if (oldPosition === newPosition) return;
	await db.transaction(async (tx) => {
		await lockPlaylist(tx as unknown as DrizzleDb, playlistId);
		await tx.update(playlistEntries).set({ position: -1 }).where(eq(playlistEntries.id, entryId));
		if (newPosition < oldPosition) {
			// Moving earlier: increment positions [newPosition, oldPosition) in descending order to avoid constraint violation
			const entriesToShift = await tx
				.select()
				.from(playlistEntries)
				.where(
					and(
						eq(playlistEntries.playlistId, playlistId),
						gte(playlistEntries.position, newPosition),
						lt(playlistEntries.position, oldPosition)
					)
				)
				.orderBy(sql`${playlistEntries.position} DESC`);
			for (const entry of entriesToShift) {
				await tx
					.update(playlistEntries)
					.set({ position: entry.position + 1 })
					.where(eq(playlistEntries.id, entry.id));
			}
		} else {
			// Moving later: decrement positions (oldPosition, newPosition] in ascending order to avoid constraint violation
			const entriesToShift = await tx
				.select()
				.from(playlistEntries)
				.where(
					and(
						eq(playlistEntries.playlistId, playlistId),
						gt(playlistEntries.position, oldPosition),
						lte(playlistEntries.position, newPosition)
					)
				)
				.orderBy(sql`${playlistEntries.position} ASC`);
			for (const entry of entriesToShift) {
				await tx
					.update(playlistEntries)
					.set({ position: entry.position - 1 })
					.where(eq(playlistEntries.id, entry.id));
			}
		}
		await tx
			.update(playlistEntries)
			.set({ position: newPosition })
			.where(eq(playlistEntries.id, entryId));
	});
}

export async function listEntries(db: DrizzleDb, playlistId: number) {
	return db
		.select()
		.from(playlistEntries)
		.where(eq(playlistEntries.playlistId, playlistId))
		.orderBy(playlistEntries.position);
}
