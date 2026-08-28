import { extname, join } from 'node:path';
import { mkdir, writeFile, unlink } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { and, eq, sql } from 'drizzle-orm';
import { artistCovers, items as itemsTable } from '../db/schema';
import type { DrizzleDb } from '../db';
import { ApiError } from '../api/errors';

export const IMAGE_TYPES: Record<string, string> = {
	'image/jpeg': '.jpg',
	'image/png': '.png',
	'image/webp': '.webp',
	'image/gif': '.gif'
};

export const MAX_IMAGE_BYTES = 5 * 1024 * 1024;

export interface ArtistCover {
	artist: string;
	itemId: number | null;
	imagePath: string | null;
}

export function imageFileName(artist: string, extension: string): string {
	return `${createHash('sha256').update(artist).digest('hex').slice(0, 32)}${extension}`;
}

export async function chosenCovers(db: DrizzleDb): Promise<Map<string, ArtistCover>> {
	const rows = await db.select().from(artistCovers);
	return new Map(
		rows.map((row) => [
			row.artist,
			{ artist: row.artist, itemId: row.itemId, imagePath: row.imagePath }
		])
	);
}

export async function chosenCover(db: DrizzleDb, artist: string): Promise<ArtistCover | null> {
	const [row] = await db.select().from(artistCovers).where(eq(artistCovers.artist, artist));
	return row ? { artist: row.artist, itemId: row.itemId, imagePath: row.imagePath } : null;
}

export async function fallbackCovers(db: DrizzleDb): Promise<Map<string, number>> {
	const rows = await db.execute(sql`
		SELECT DISTINCT ON (artist) artist, id
		FROM items
		WHERE kind = 'album' AND artist IS NOT NULL AND cover_path IS NOT NULL
		ORDER BY artist, md5(artist || ':' || id::text)
	`);
	const map = new Map<string, number>();
	for (const row of rows as unknown as Array<{ artist: string; id: number }>) {
		map.set(row.artist, row.id);
	}
	return map;
}

export async function albumsOf(db: DrizzleDb, artist: string) {
	return db
		.select({
			id: itemsTable.id,
			title: itemsTable.title,
			year: itemsTable.year,
			coverPath: itemsTable.coverPath
		})
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'album'), eq(itemsTable.artist, artist)))
		.orderBy(sql`${itemsTable.year} DESC NULLS LAST`, sql`lower(${itemsTable.sortTitle})`);
}

async function replace(db: DrizzleDb, artist: string, values: Partial<ArtistCover>) {
	const previous = await chosenCover(db, artist);
	await db
		.insert(artistCovers)
		.values({
			artist,
			itemId: values.itemId ?? null,
			imagePath: values.imagePath ?? null,
			updatedAt: sql`now()`
		})
		.onConflictDoUpdate({
			target: artistCovers.artist,
			set: {
				itemId: values.itemId ?? null,
				imagePath: values.imagePath ?? null,
				updatedAt: sql`now()`
			}
		});
	if (previous?.imagePath && previous.imagePath !== values.imagePath) {
		await unlink(previous.imagePath).catch(() => undefined);
	}
}

export async function selectAlbum(db: DrizzleDb, artist: string, itemId: number): Promise<void> {
	const [album] = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(
			and(eq(itemsTable.id, itemId), eq(itemsTable.kind, 'album'), eq(itemsTable.artist, artist))
		);
	if (!album) throw new ApiError(422, 'Album gehört nicht zu diesem Interpreten');
	await replace(db, artist, { itemId });
}

export async function storeImage(
	db: DrizzleDb,
	artist: string,
	file: { type: string; size: number; bytes: Uint8Array },
	artistsDir: string
): Promise<string> {
	const extension = IMAGE_TYPES[file.type];
	if (!extension) throw new ApiError(422, 'Nicht unterstütztes Bildformat');
	if (file.size > MAX_IMAGE_BYTES) throw new ApiError(413, 'Bild ist zu groß (maximal 5 MB)');
	if (file.size === 0) throw new ApiError(422, 'Leere Datei');

	await mkdir(artistsDir, { recursive: true });
	const destination = join(artistsDir, imageFileName(artist, extension));
	await writeFile(destination, file.bytes);
	await replace(db, artist, { imagePath: destination });
	return destination;
}

export async function clearCover(db: DrizzleDb, artist: string): Promise<void> {
	const previous = await chosenCover(db, artist);
	await db.delete(artistCovers).where(eq(artistCovers.artist, artist));
	if (previous?.imagePath) await unlink(previous.imagePath).catch(() => undefined);
}

export function imageContentType(path: string): string {
	const extension = extname(path).toLowerCase();
	const found = Object.entries(IMAGE_TYPES).find(([, value]) => value === extension);
	return found ? found[0] : 'application/octet-stream';
}
