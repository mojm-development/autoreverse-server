import { describe, it, expect } from 'vitest';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withTestDb } from '../fixtures';
import { items as itemsTable, artistCovers } from '../../src/lib/server/db/schema';
import {
	chosenCover,
	chosenCovers,
	fallbackCovers,
	albumsOf,
	selectAlbum,
	storeImage,
	clearCover,
	imageFileName,
	imageContentType
} from '../../src/lib/server/library/artistCovers';
import { ApiError } from '../../src/lib/server/api/errors';
import type { DrizzleDb } from '../../src/lib/server/db';
import { createUser } from '../../src/lib/server/auth/passwords';
import { callRoute } from './_callRoute';
import { _artistImageGetHandler } from '../../src/routes/artists/[name]/image/+server';
import { _artistCoverGetHandler } from '../../src/routes/artists/[name]/cover/+server';

async function seedAlbum(db: DrizzleDb, artist: string, title: string, withCover = true) {
	const [row] = await db
		.insert(itemsTable)
		.values({
			kind: 'album',
			title,
			sortTitle: title.toLowerCase(),
			artist,
			coverPath: withCover ? `/covers/${title}.jpg` : null
		})
		.returning();
	return row;
}

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

describe('artist cover fallback', () => {
	it('picks the same album every time for a given artist', async () => {
		await withTestDb(async (db) => {
			for (const title of ['13', 'Jazz ist anders', 'Die Bestie in Menschengestalt']) {
				await seedAlbum(db, 'Die Ärzte', title);
			}
			const first = await fallbackCovers(db);
			const second = await fallbackCovers(db);
			expect(first.get('Die Ärzte')).toBeDefined();
			expect(second.get('Die Ärzte')).toBe(first.get('Die Ärzte'));
		});
	});

	it('offers one album per artist', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, 'Die Ärzte', '13');
			await seedAlbum(db, 'Die Ärzte', 'Jazz ist anders');
			await seedAlbum(db, 'Bon Jovi', 'Crush');
			const map = await fallbackCovers(db);
			expect(map.size).toBe(2);
			expect(map.get('Bon Jovi')).toBeDefined();
		});
	});

	it('ignores albums that have no cover of their own', async () => {
		await withTestDb(async (db) => {
			await seedAlbum(db, 'Ansa Volt', 'Nordlicht', false);
			const map = await fallbackCovers(db);
			expect(map.has('Ansa Volt')).toBe(false);
		});
	});
});

describe('choosing an album', () => {
	it('stores the pick and reports it back', async () => {
		await withTestDb(async (db) => {
			const album = await seedAlbum(db, 'Die Ärzte', '13');
			await selectAlbum(db, 'Die Ärzte', album.id);
			const cover = await chosenCover(db, 'Die Ärzte');
			expect(cover?.itemId).toBe(album.id);
			expect(cover?.imagePath).toBeNull();
		});
	});

	it('refuses an album that belongs to a different artist', async () => {
		await withTestDb(async (db) => {
			const album = await seedAlbum(db, 'Bon Jovi', 'Crush');
			await expect(selectAlbum(db, 'Die Ärzte', album.id)).rejects.toBeInstanceOf(ApiError);
		});
	});

	it('replaces an earlier pick rather than adding a second row', async () => {
		await withTestDb(async (db) => {
			const first = await seedAlbum(db, 'Die Ärzte', '13');
			const second = await seedAlbum(db, 'Die Ärzte', 'Jazz ist anders');
			await selectAlbum(db, 'Die Ärzte', first.id);
			await selectAlbum(db, 'Die Ärzte', second.id);
			const rows = await db.select().from(artistCovers);
			expect(rows).toHaveLength(1);
			expect(rows[0].itemId).toBe(second.id);
		});
	});

	it('is dropped when the album it points at is deleted', async () => {
		await withTestDb(async (db) => {
			const album = await seedAlbum(db, 'Die Ärzte', '13');
			await selectAlbum(db, 'Die Ärzte', album.id);
			await db.delete(itemsTable);
			expect(await chosenCover(db, 'Die Ärzte')).toBeNull();
		});
	});

	it('lists an artist albums newest first for the picker', async () => {
		await withTestDb(async (db) => {
			await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'Alt', sortTitle: 'alt', artist: 'X', year: 1990 });
			await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'Neu', sortTitle: 'neu', artist: 'X', year: 2020 });
			const albums = await albumsOf(db, 'X');
			expect(albums.map((a) => a.title)).toEqual(['Neu', 'Alt']);
		});
	});
});

describe('uploading an image', () => {
	it('writes the file and points the artist at it', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-artists-'));
			const path = await storeImage(
				db,
				'Die Ärzte',
				{ type: 'image/png', size: PNG.length, bytes: PNG },
				dir
			);
			expect(path).toBe(join(dir, imageFileName('Die Ärzte', '.png')));
			expect(new Uint8Array(await readFile(path))).toEqual(PNG);
			const cover = await chosenCover(db, 'Die Ärzte');
			expect(cover?.imagePath).toBe(path);
			expect(cover?.itemId).toBeNull();
		});
	});

	it('rejects a format it cannot serve, and an oversized file', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-artists-'));
			await expect(
				storeImage(db, 'X', { type: 'application/pdf', size: 10, bytes: PNG }, dir)
			).rejects.toBeInstanceOf(ApiError);
			await expect(
				storeImage(db, 'X', { type: 'image/png', size: 6 * 1024 * 1024, bytes: PNG }, dir)
			).rejects.toBeInstanceOf(ApiError);
		});
	});

	it('an upload replaces an album pick, and clearing removes the file', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-artists-'));
			const album = await seedAlbum(db, 'Die Ärzte', '13');
			await selectAlbum(db, 'Die Ärzte', album.id);
			const path = await storeImage(
				db,
				'Die Ärzte',
				{ type: 'image/png', size: PNG.length, bytes: PNG },
				dir
			);
			const afterUpload = await chosenCover(db, 'Die Ärzte');
			expect(afterUpload?.itemId).toBeNull();

			await clearCover(db, 'Die Ärzte');
			expect(await chosenCover(db, 'Die Ärzte')).toBeNull();
			await expect(stat(path)).rejects.toThrow();
		});
	});

	it('chosenCovers returns every artist in one map', async () => {
		await withTestDb(async (db) => {
			const a = await seedAlbum(db, 'A', 'a1');
			const b = await seedAlbum(db, 'B', 'b1');
			await selectAlbum(db, 'A', a.id);
			await selectAlbum(db, 'B', b.id);
			const map = await chosenCovers(db);
			expect(map.size).toBe(2);
			expect(map.get('A')?.itemId).toBe(a.id);
		});
	});
});

describe('imageContentType', () => {
	it('maps stored extensions back to their media type', () => {
		expect(imageContentType('/data/artists/abc.png')).toBe('image/png');
		expect(imageContentType('/data/artists/abc.jpg')).toBe('image/jpeg');
		expect(imageContentType('/data/artists/abc.txt')).toBe('application/octet-stream');
	});
});

describe('the artist image route', () => {
	// Die Route hieß immer schon „image", lieferte aber nur hochgeladene Dateien: Wer
	// ein Album ausgewählt hatte, bekam 404. Das fiel erst auf, als ein Client kam,
	// der die Auswahl nicht selbst auflösen kann — die Weboberfläche tut das in ihrem
	// Seitenlader und merkte davon nichts.
	it('serves the chosen album cover, not just an uploaded file', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-cover-'));
			const coverPath = join(dir, 'album.png');
			writeFileSync(coverPath, Buffer.from(PNG));
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [album] = await db
				.insert(itemsTable)
				.values({
					kind: 'album',
					title: 'Autobahn',
					sortTitle: 'autobahn',
					artist: 'Kraftwerk',
					coverPath
				})
				.returning();
			await selectAlbum(db, 'Kraftwerk', album.id);

			const res = await callRoute(_artistImageGetHandler, {
				db,
				locals: { userId, token: null },
				params: { name: 'Kraftwerk' }
			});
			expect(res.status).toBe(200);
			expect(res.headers.get('content-type')).toBe('image/png');
		});
	});

	it('falls back to the derived album when nothing was chosen', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-cover-'));
			const coverPath = join(dir, 'album.png');
			writeFileSync(coverPath, Buffer.from(PNG));
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await db.insert(itemsTable).values({
				kind: 'album',
				title: 'Autobahn',
				sortTitle: 'autobahn',
				artist: 'Kraftwerk',
				coverPath
			});

			const res = await callRoute(_artistImageGetHandler, {
				db,
				locals: { userId, token: null },
				params: { name: 'Kraftwerk' }
			});
			expect(res.status).toBe(200);
		});
	});

	it('still says 404 for an artist without any cover at all', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await seedAlbum(db, 'Kraftwerk', 'Autobahn', false);
			const res = await callRoute(_artistImageGetHandler, {
				db,
				locals: { userId, token: null },
				params: { name: 'Kraftwerk' }
			});
			expect(res.status).toBe(404);
		});
	});
});

describe('reading back the choice', () => {
	// Ohne diese Route sieht ein Client zwar das Bild, aber nicht seine Herkunft — und
	// könnte im Auswahlbildschirm nicht zeigen, welches Album gerade gilt.
	it('names the chosen album', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const album = await seedAlbum(db, 'Kraftwerk', 'Autobahn');
			await selectAlbum(db, 'Kraftwerk', album.id);
			const res = await callRoute(_artistCoverGetHandler, {
				db,
				locals: { userId, token: null },
				params: { name: 'Kraftwerk' }
			});
			expect(res.status).toBe(200);
			expect(await res.json()).toEqual({
				artist: 'Kraftwerk',
				item_id: album.id,
				has_image: false
			});
		});
	});

	it('reports an uploaded image without an album id', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-artists-'));
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await storeImage(db, 'Kraftwerk', { type: 'image/png', size: PNG.length, bytes: PNG }, dir);
			const res = await callRoute(_artistCoverGetHandler, {
				db,
				locals: { userId, token: null },
				params: { name: 'Kraftwerk' }
			});
			expect(await res.json()).toEqual({
				artist: 'Kraftwerk',
				item_id: null,
				has_image: true
			});
		});
	});

	it('says nothing is chosen when nothing is', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_artistCoverGetHandler, {
				db,
				locals: { userId, token: null },
				params: { name: 'Kraftwerk' }
			});
			expect(await res.json()).toEqual({
				artist: 'Kraftwerk',
				item_id: null,
				has_image: false
			});
		});
	});
});
