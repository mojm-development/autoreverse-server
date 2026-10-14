import { describe, it, expect } from 'vitest';
import { eq } from 'drizzle-orm';
import { withTestDb } from '../fixtures';
import { items as itemsTable, metadataEdits } from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';
import { bulkEdit, undoBatch, BULK_LIMIT } from '../../src/lib/server/library/bulkMetadata';
import { callRoute } from './_callRoute';
import { _itemsBulkPostHandler } from '../../src/routes/items/bulk/+server';
import { _itemsBulkUndoPostHandler } from '../../src/routes/items/bulk/undo/+server';
import { editItem } from '../../src/lib/server/library/metadata';

async function seedAlbums(db: Parameters<typeof bulkEdit>[0]) {
	return db
		.insert(itemsTable)
		.values([
			{
				kind: 'album',
				title: 'Live in Berlin (Remaster)',
				sortTitle: 'live in berlin (remaster)',
				artist: 'ACDC'
			},
			{
				kind: 'album',
				title: 'Back in Black (Remaster)',
				sortTitle: 'back in black (remaster)',
				artist: 'ACDC'
			},
			{ kind: 'album', title: 'Nevermind', sortTitle: 'nevermind', artist: 'Nirvana' }
		])
		.returning();
}

describe('bulk metadata editing', () => {
	it('a dry run answers with the diff and writes nothing', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const [first, second] = await seedAlbums(db);

			const preview = await bulkEdit(db, userId, {
				ids: [first.id, second.id],
				set: { artist: 'AC/DC' }
			});
			expect(preview.matched).toBe(2);
			expect(preview.changed).toBe(2);
			expect(preview.batch_id).toBeNull();
			expect(preview.changes[0].fields).toEqual([{ field: 'artist', old: 'ACDC', new: 'AC/DC' }]);

			const [unchanged] = await db.select().from(itemsTable).where(eq(itemsTable.id, first.id));
			expect(unchanged.artist).toBe('ACDC');
			expect(await db.select().from(metadataEdits)).toHaveLength(0);
		});
	}, 60_000);

	it('applies a search-and-replace across a filter and locks what it wrote', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await seedAlbums(db);

			const result = await bulkEdit(db, userId, {
				filter: { kind: 'album', artist: 'ACDC' },
				replace: { field: 'title', from: ' (Remaster)', to: '' },
				dry_run: false
			});
			expect(result.changed).toBe(2);
			expect(result.batch_id).not.toBeNull();

			const rows = await db.select().from(itemsTable).orderBy(itemsTable.title);
			expect(rows.map((r) => r.title)).toEqual(['Back in Black', 'Live in Berlin', 'Nevermind']);
			// The sort key follows the title, and both are locked against the scanner.
			const edited = rows.find((r) => r.title === 'Back in Black')!;
			expect(edited.sortTitle).toBe('back in black');
			expect(edited.lockedFields).toEqual(['sortTitle', 'title']);
			// The album the filter did not match is untouched.
			expect(rows.find((r) => r.title === 'Nevermind')!.lockedFields).toEqual([]);
		});
	}, 60_000);

	it('undo restores the values and the locks that came with them', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const [first] = await seedAlbums(db);

			const applied = await bulkEdit(db, userId, {
				ids: [first.id],
				set: { artist: 'AC/DC', year: 1980 },
				dry_run: false
			});
			const undone = await undoBatch(db, applied.batch_id!);
			expect(undone.skipped).toBe(0);
			expect(undone.restored).toBe(2);

			const [row] = await db.select().from(itemsTable).where(eq(itemsTable.id, first.id));
			expect(row.artist).toBe('ACDC');
			expect(row.year).toBeNull();
			// Locks go back too, or the restored value would stay frozen.
			expect(row.lockedFields).toEqual([]);
			expect(await db.select().from(metadataEdits)).toHaveLength(0);
		});
	}, 60_000);

	it('undo leaves alone what someone changed after the batch', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const [first] = await seedAlbums(db);

			const applied = await bulkEdit(db, userId, {
				ids: [first.id],
				set: { artist: 'AC/DC', year: 1980 },
				dry_run: false
			});
			// Someone corrects the artist again by hand before pressing undo.
			await editItem(db, userId, first.id, { set: { artist: 'AC/DC (live)' } });

			const undone = await undoBatch(db, applied.batch_id!);
			expect(undone.skipped).toBe(1);
			const [row] = await db.select().from(itemsTable).where(eq(itemsTable.id, first.id));
			expect(row.artist).toBe('AC/DC (live)');
			// The untouched field still rolls back.
			expect(row.year).toBeNull();
		});
	}, 60_000);

	it('refuses an ambiguous or oversized request instead of guessing', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const [first] = await seedAlbums(db);

			await expect(
				bulkEdit(db, userId, { ids: [first.id], filter: { kind: 'album' }, set: { year: 1 } })
			).rejects.toThrow('Entweder ids oder filter, nicht beides');
			await expect(bulkEdit(db, userId, { ids: [first.id] })).rejects.toThrow(
				'Keine Änderungen angegeben'
			);
			await expect(
				bulkEdit(db, userId, {
					ids: Array.from({ length: BULK_LIMIT + 1 }, (_, i) => i + 1),
					set: { year: 1 }
				})
			).rejects.toThrow(`Höchstens ${BULK_LIMIT} Items auf einmal`);
			await expect(
				bulkEdit(db, userId, {
					ids: [first.id],
					replace: { field: 'title', from: '(', to: '', regex: true }
				})
			).rejects.toThrow('Ungültiger regulärer Ausdruck');
			await expect(
				bulkEdit(db, userId, { ids: [first.id], replace: { field: 'year', from: '1', to: '2' } })
			).rejects.toThrow('Ersetzen geht nur in Textfeldern');
			await expect(undoBatch(db, 'gibt-es-nicht')).rejects.toThrow('Unbekannte Änderung');
		});
	}, 60_000);

	it('counts what matched even when most of it needs no change', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await seedAlbums(db);

			const preview = await bulkEdit(db, userId, {
				filter: { kind: 'album' },
				set: { artist: 'ACDC' }
			});
			expect(preview.matched).toBe(3);
			// Two already carry that artist: nothing to change there, but the field does
			// come off the scanner — reported separately rather than as a diff.
			expect(preview.changed).toBe(1);
			expect(preview.locked_only).toBe(2);
			expect(preview.changes).toHaveLength(1);
			expect(preview.changes[0].title).toBe('Nevermind');
		});
	}, 60_000);

	it('the endpoints are for Verwalter, and the undo route needs a batch id', async () => {
		await withTestDb(async (db) => {
			const admin = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const guest = await createUser(db, 'gast', 'hunter2hunter2', false);
			const [first] = await seedAlbums(db);

			const refused = await callRoute(_itemsBulkPostHandler, {
				db,
				locals: { userId: guest, token: null },
				body: { ids: [first.id], set: { artist: 'X' } }
			});
			expect(refused.status).toBe(403);

			const preview = await callRoute(_itemsBulkPostHandler, {
				db,
				locals: { userId: admin, token: null },
				body: { ids: [first.id], set: { artist: 'AC/DC' } }
			});
			expect(preview.status).toBe(200);
			const payload = await preview.json();
			// The route defaults to a dry run: an omitted dry_run must not write.
			expect(payload.batch_id).toBeNull();
			expect(payload.changed).toBe(1);

			const missingId = await callRoute(_itemsBulkUndoPostHandler, {
				db,
				locals: { userId: admin, token: null },
				body: {}
			});
			expect(missingId.status).toBe(422);
			expect((await missingId.json()).detail).toBe('batch_id fehlt');
		});
	}, 60_000);
}, 60_000);
