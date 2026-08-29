import { extname, dirname } from 'node:path';
import { rename, mkdir } from 'node:fs/promises';
import { eq } from 'drizzle-orm';
import { alias } from 'drizzle-orm/pg-core';
import { items as itemsTable, tracks as tracksTable } from '../db/schema';
import { episodeDestination, fileExists, isInside } from './paths';
import type { DrizzleDb } from '../db';

export interface RelocationResult {
	moved: number;
	skipped: number;
	failed: number;
}

/**
 * Episodes downloaded before the <Podcast>/<Folge>.<ext> layout existed sit flat in the podcasts
 * root, named after their id. Move them where the downloader would put them today and carry
 * `tracks.path` along, so the old files stay playable instead of being re-downloaded.
 *
 * Left alone: anything already in place, a row whose file is gone, and any path outside the
 * podcasts root (a scanned library file has no business being moved). The extension comes from the
 * file itself rather than the feed — the feed URL may have changed since, the bytes on disk did
 * not. Runs at startup; once everything has moved it costs one query per downloaded episode and
 * no file I/O at all.
 */
export async function relocateLegacyDownloads(
	db: DrizzleDb,
	podcastsDir: string
): Promise<RelocationResult> {
	const podcast = alias(itemsTable, 'podcast');
	const rows = await db
		.select({
			trackId: tracksTable.id,
			path: tracksTable.path,
			episodeId: itemsTable.id,
			episodeTitle: itemsTable.title,
			podcastTitle: podcast.title
		})
		.from(tracksTable)
		.innerJoin(itemsTable, eq(itemsTable.id, tracksTable.itemId))
		.innerJoin(podcast, eq(podcast.id, itemsTable.parentId))
		.where(eq(itemsTable.kind, 'episode'));

	const result: RelocationResult = { moved: 0, skipped: 0, failed: 0 };
	for (const row of rows) {
		if (!isInside(row.path, podcastsDir) || !(await fileExists(row.path))) {
			result.skipped += 1;
			continue;
		}
		const destination = await episodeDestination(
			db,
			podcastsDir,
			{ id: row.episodeId, title: row.episodeTitle },
			row.podcastTitle,
			extname(row.path) || '.mp3'
		);
		if (destination === row.path) {
			result.skipped += 1;
			continue;
		}
		try {
			await mkdir(dirname(destination), { recursive: true });
			await rename(row.path, destination);
		} catch {
			// A failed move leaves the row pointing at the file that is still there.
			result.failed += 1;
			continue;
		}
		await db.update(tracksTable).set({ path: destination }).where(eq(tracksTable.id, row.trackId));
		result.moved += 1;
	}
	return result;
}
