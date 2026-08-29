import { unlink } from 'node:fs/promises';
import { and, eq, sql } from 'drizzle-orm';
import { items as itemsTable, tracks as tracksTable, libraryConfig } from '../db/schema';
import type { DrizzleDb } from '../db';
import { downloadEpisode, pruneEmptyPodcastDir } from './download';
import { refresh } from './store';

export const KEEP_MIN = 0;
export const KEEP_MAX = 50;

export const clampKeep = (value: number) =>
	Math.min(KEEP_MAX, Math.max(KEEP_MIN, Math.trunc(value)));

export interface RetentionResult {
	downloaded: number;
	freed: number;
	failed: number;
}

export async function getKeepDefault(db: DrizzleDb): Promise<number> {
	const [row] = await db
		.select({ keep: libraryConfig.podcastKeepEpisodes })
		.from(libraryConfig)
		.where(eq(libraryConfig.id, 1));
	return row?.keep ?? 0;
}

export async function setKeepDefault(db: DrizzleDb, keep: number): Promise<void> {
	const value = clampKeep(keep);
	await db
		.insert(libraryConfig)
		.values({ id: 1, podcastKeepEpisodes: value })
		.onConflictDoUpdate({
			target: libraryConfig.id,
			set: { podcastKeepEpisodes: value, updatedAt: sql`now()` }
		});
}

export async function setKeepForPodcast(
	db: DrizzleDb,
	podcastId: number,
	keep: number | null
): Promise<void> {
	await db
		.update(itemsTable)
		.set({ keepEpisodes: keep === null ? null : clampKeep(keep) })
		.where(and(eq(itemsTable.id, podcastId), eq(itemsTable.kind, 'podcast')));
}

export async function effectiveKeep(
	db: DrizzleDb,
	podcast: { keepEpisodes: number | null }
): Promise<number> {
	return podcast.keepEpisodes ?? (await getKeepDefault(db));
}

async function removeDownload(
	db: DrizzleDb,
	episodeId: number,
	podcastsDir: string
): Promise<number> {
	const rows = await db
		.delete(tracksTable)
		.where(eq(tracksTable.itemId, episodeId))
		.returning({ path: tracksTable.path });
	for (const row of rows) {
		await unlink(row.path).catch(() => undefined);
		await pruneEmptyPodcastDir(row.path, podcastsDir);
	}
	return rows.length;
}

export async function applyRetention(
	db: DrizzleDb,
	podcastId: number,
	keep: number,
	podcastsDir: string
): Promise<RetentionResult> {
	const episodes = await db.execute(sql`
		SELECT episode.id,
		       (SELECT count(*) FROM tracks WHERE tracks.item_id = episode.id) > 0 AS downloaded
		FROM items AS episode
		WHERE episode.parent_id = ${podcastId} AND episode.kind = 'episode'
		ORDER BY episode.published_at DESC NULLS LAST, episode.id DESC
	`);
	const ordered = episodes as unknown as Array<{ id: number; downloaded: boolean }>;

	const result: RetentionResult = { downloaded: 0, freed: 0, failed: 0 };
	const wanted = ordered.slice(0, keep);
	const surplus = ordered.slice(keep);

	for (const episode of wanted) {
		if (episode.downloaded) continue;
		try {
			await downloadEpisode(db, episode.id, podcastsDir);
			result.downloaded += 1;
		} catch {
			result.failed += 1;
		}
	}
	for (const episode of surplus) {
		if (!episode.downloaded) continue;
		result.freed += await removeDownload(db, episode.id, podcastsDir);
	}
	return result;
}

export async function retainForPodcast(
	db: DrizzleDb,
	podcastId: number,
	podcastsDir: string
): Promise<RetentionResult> {
	const [podcast] = await db
		.select({ id: itemsTable.id, keepEpisodes: itemsTable.keepEpisodes })
		.from(itemsTable)
		.where(and(eq(itemsTable.id, podcastId), eq(itemsTable.kind, 'podcast')));
	if (!podcast) return { downloaded: 0, freed: 0, failed: 0 };
	const keep = await effectiveKeep(db, podcast);
	return applyRetention(db, podcast.id, keep, podcastsDir);
}

export async function refreshAllAndRetain(
	db: DrizzleDb,
	dirs: { coversDir: string; podcastsDir: string }
): Promise<{ podcasts: number; downloaded: number; freed: number; failed: number }> {
	const podcasts = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(eq(itemsTable.kind, 'podcast'));

	const totals = { podcasts: 0, downloaded: 0, freed: 0, failed: 0 };
	for (const podcast of podcasts) {
		totals.podcasts += 1;
		try {
			await refresh(db, podcast.id, { coversDir: dirs.coversDir });
		} catch {
			totals.failed += 1;
			continue;
		}
		const result = await retainForPodcast(db, podcast.id, dirs.podcastsDir);
		totals.downloaded += result.downloaded;
		totals.freed += result.freed;
		totals.failed += result.failed;
	}
	return totals;
}
