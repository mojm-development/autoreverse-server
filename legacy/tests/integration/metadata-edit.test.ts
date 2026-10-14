import { describe, it, expect } from 'vitest';
import { eq } from 'drizzle-orm';
import { withTestDb } from '../fixtures';
import {
	items as itemsTable,
	tracks as tracksTable,
	metadataEdits
} from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';
import { editItem, editTrack } from '../../src/lib/server/library/metadata';
import { storeItems } from '../../src/lib/server/scanner/store';
import type { ScannedItem } from '../../src/lib/server/scanner/books';
import { callRoute } from './_callRoute';
import { _itemPatchHandler } from '../../src/routes/items/[id]/+server';

function scanned(overrides: Partial<ScannedItem> = {}): ScannedItem {
	return {
		sourcePath: '/library/books/Autor/Buch',
		kind: 'book',
		title: 'Titel aus der Datei',
		author: 'Autor aus der Datei',
		series: null,
		seriesIndex: null,
		tracks: [
			{
				path: '/library/books/Autor/Buch/01.mp3',
				position: 1,
				title: 'Track aus der Datei',
				disc: null,
				duration: 10,
				mtime: 100,
				size: 200
			}
		],
		chapters: [{ title: 'K', start: 0, end: 10 }],
		unchanged: false,
		...overrides
	};
}

describe('metadata editing', () => {
	it('locks what a person set, so the next scan cannot undo it', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await storeItems(db, [scanned()], '/library/books', '/data/covers');
			const [stored] = await db.select().from(itemsTable);

			await editItem(db, userId, stored.id, {
				set: { title: 'Der richtige Titel', narrator: 'Anna Sprecherin' }
			});

			// A changed file makes the scanner rewrite the folder — the correction stays.
			await storeItems(
				db,
				[
					scanned({
						title: 'Titel aus der Datei',
						author: 'Neuer Autor aus der Datei',
						tracks: [
							{
								path: '/library/books/Autor/Buch/01.mp3',
								position: 1,
								title: 'Track aus der Datei',
								disc: null,
								duration: 10,
								mtime: 999,
								size: 999
							}
						]
					})
				],
				'/library/books',
				'/data/covers'
			);

			const [after] = await db.select().from(itemsTable).where(eq(itemsTable.id, stored.id));
			expect(after.title).toBe('Der richtige Titel');
			expect(after.narrator).toBe('Anna Sprecherin');
			// The sort key follows a locked title rather than falling back to the file's.
			expect(after.sortTitle).toBe('der richtige titel');
			// Everything untouched still belongs to the scanner.
			expect(after.author).toBe('Neuer Autor aus der Datei');
		});
	}, 60_000);

	it('hands a field back to the scanner when it is reset', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await storeItems(db, [scanned()], '/library/books', '/data/covers');
			const [stored] = await db.select().from(itemsTable);

			await editItem(db, userId, stored.id, { set: { title: 'Handkorrektur' } });
			const reset = await editItem(db, userId, stored.id, { reset: ['title'] });
			expect(reset.row.lockedFields).toEqual([]);

			await storeItems(
				db,
				[
					scanned({
						title: 'Titel aus der Datei',
						tracks: [
							{
								path: '/library/books/Autor/Buch/01.mp3',
								position: 1,
								title: 'T',
								disc: null,
								duration: 10,
								mtime: 998,
								size: 998
							}
						]
					})
				],
				'/library/books',
				'/data/covers'
			);
			const [after] = await db.select().from(itemsTable).where(eq(itemsTable.id, stored.id));
			expect(after.title).toBe('Titel aus der Datei');
		});
	}, 60_000);

	it('records every change with the value it replaced, under one batch id', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await storeItems(db, [scanned()], '/library/books', '/data/covers');
			const [stored] = await db.select().from(itemsTable);

			const result = await editItem(db, userId, stored.id, {
				set: { title: 'Neu', year: 2011 }
			});
			const edits = await db
				.select()
				.from(metadataEdits)
				.where(eq(metadataEdits.batchId, result.batchId));
			const byField = Object.fromEntries(edits.map((e) => [e.field, e]));
			expect(byField.title.oldValue).toBe('Titel aus der Datei');
			expect(byField.title.newValue).toBe('Neu');
			expect(byField.year.oldValue).toBeNull();
			expect(byField.year.newValue).toBe('2011');
			// The derived sort key is logged as well, or an undo could not restore it.
			expect(byField.sortTitle.newValue).toBe('neu');
			expect(edits.every((e) => e.userId === userId)).toBe(true);
		});
	}, 60_000);

	it('clears a field on null and keeps it clear, and refuses nonsense', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await storeItems(
				db,
				[scanned({ author: 'Falscher Autor' })],
				'/library/books',
				'/data/covers'
			);
			const [stored] = await db.select().from(itemsTable);

			const cleared = await editItem(db, userId, stored.id, { set: { author: null } });
			expect(cleared.row.author).toBeNull();
			expect(cleared.row.lockedFields).toContain('author');

			await expect(editItem(db, userId, stored.id, { set: { kind: 'album' } })).rejects.toThrow(
				'Unbekanntes Feld: kind'
			);
			await expect(editItem(db, userId, stored.id, { set: { year: 'bald' } })).rejects.toThrow(
				'year muss eine Zahl sein'
			);
			await expect(editItem(db, userId, stored.id, {})).rejects.toThrow(
				'Keine Änderungen angegeben'
			);
		});
	}, 60_000);

	it('edits a track title and locks it too', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			await storeItems(db, [scanned()], '/library/books', '/data/covers');
			const [track] = await db.select().from(tracksTable);

			const result = await editTrack(db, userId, track.id, { set: { title: 'Kapitel eins' } });
			expect(result.row.title).toBe('Kapitel eins');
			expect(result.row.lockedFields).toEqual(['title']);
		});
	}, 60_000);

	it('PATCH /items/[id] edits for a Verwalter and refuses everyone else', async () => {
		await withTestDb(async (db) => {
			// The very first user is always an admin, so the guest needs company.
			const admin = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const guest = await createUser(db, 'gast', 'hunter2hunter2', false);
			await storeItems(db, [scanned()], '/library/books', '/data/covers');
			const [stored] = await db.select().from(itemsTable);

			const refused = await callRoute(_itemPatchHandler, {
				db,
				locals: { userId: guest, token: null },
				params: { id: String(stored.id) },
				body: { set: { title: 'Nicht erlaubt' } }
			});
			expect(refused.status).toBe(403);
			expect((await refused.json()).detail).toBe('Nur für Verwalter');

			const allowed = await callRoute(_itemPatchHandler, {
				db,
				locals: { userId: admin, token: null },
				params: { id: String(stored.id) },
				body: { set: { title: 'Erlaubt' } }
			});
			expect(allowed.status).toBe(200);
			const payload = await allowed.json();
			expect(payload.title).toBe('Erlaubt');
			expect(payload.locked_fields).toEqual(['sortTitle', 'title']);
			expect(payload.changed).toContain('title');
			expect(typeof payload.batch_id).toBe('string');
		});
	}, 60_000);
}, 60_000);
