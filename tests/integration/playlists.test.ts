import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import {
	items as itemsTable,
	playlists as playlistsTable,
	playlistEntries
} from '../../src/lib/server/db/schema';
import type { DrizzleDb } from '../../src/lib/server/db';
import { eq, asc } from 'drizzle-orm';
import { appendEntry, removeEntry, moveEntry } from '../../src/lib/server/library/playlistEntries';

async function positions(db: DrizzleDb, playlistId: number) {
	const rows = await db
		.select()
		.from(playlistEntries)
		.where(eq(playlistEntries.playlistId, playlistId))
		.orderBy(asc(playlistEntries.position));
	return rows.map((r) => ({ id: r.id, position: r.position }));
}

describe('playlist gapless reorder', () => {
	it('append assigns 1, 2, 3, ...', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [playlist] = await db.insert(playlistsTable).values({ userId, name: 'X' }).returning();
			const [track] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			await appendEntry(db, playlist.id, { itemId: track.id });
			await appendEntry(db, playlist.id, { itemId: track.id });
			await appendEntry(db, playlist.id, { itemId: track.id });
			expect((await positions(db, playlist.id)).map((r) => r.position)).toEqual([1, 2, 3]);
		});
	});

	it('remove closes the gap: removing position 2 of 3 shifts position 3 down to 2', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [playlist] = await db.insert(playlistsTable).values({ userId, name: 'X' }).returning();
			const [track] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			const e1 = await appendEntry(db, playlist.id, { itemId: track.id });
			const e2 = await appendEntry(db, playlist.id, { itemId: track.id });
			const e3 = await appendEntry(db, playlist.id, { itemId: track.id });
			await removeEntry(db, playlist.id, e2, 2);
			const remaining = await positions(db, playlist.id);
			expect(remaining).toEqual([
				{ id: e1, position: 1 },
				{ id: e3, position: 2 }
			]);
		});
	});

	it('move earlier shifts the range [new, old) up by one', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [playlist] = await db.insert(playlistsTable).values({ userId, name: 'X' }).returning();
			const [track] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			const e1 = await appendEntry(db, playlist.id, { itemId: track.id }); // pos 1
			const e2 = await appendEntry(db, playlist.id, { itemId: track.id }); // pos 2
			const e3 = await appendEntry(db, playlist.id, { itemId: track.id }); // pos 3
			await moveEntry(db, playlist.id, e3, 3, 1); // move last to first
			const rows = await positions(db, playlist.id);
			expect(rows.find((r) => r.id === e3)!.position).toBe(1);
			expect(rows.find((r) => r.id === e1)!.position).toBe(2);
			expect(rows.find((r) => r.id === e2)!.position).toBe(3);
		});
	});

	it('move later shifts the range (old, new] down by one', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [playlist] = await db.insert(playlistsTable).values({ userId, name: 'X' }).returning();
			const [track] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			const e1 = await appendEntry(db, playlist.id, { itemId: track.id }); // pos 1
			const e2 = await appendEntry(db, playlist.id, { itemId: track.id }); // pos 2
			const e3 = await appendEntry(db, playlist.id, { itemId: track.id }); // pos 3
			await moveEntry(db, playlist.id, e1, 1, 3); // move first to last
			const rows = await positions(db, playlist.id);
			expect(rows.find((r) => r.id === e1)!.position).toBe(3);
			expect(rows.find((r) => r.id === e2)!.position).toBe(1);
			expect(rows.find((r) => r.id === e3)!.position).toBe(2);
		});
	});

	it('move to the same position is a no-op', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [playlist] = await db.insert(playlistsTable).values({ userId, name: 'X' }).returning();
			const [track] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			const e1 = await appendEntry(db, playlist.id, { itemId: track.id });
			await moveEntry(db, playlist.id, e1, 1, 1);
			expect((await positions(db, playlist.id))[0]).toEqual({ id: e1, position: 1 });
		});
	});
}, 60_000);
