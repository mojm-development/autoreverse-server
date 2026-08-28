import { describe, it, expect } from 'vitest';
import { mkdtempSync } from 'node:fs';
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
