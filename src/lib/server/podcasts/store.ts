import { and, eq, isNotNull } from 'drizzle-orm';
import { items as itemsTable, tracks as tracksTable } from '../db/schema';
import { saveEpisodeChapters } from './chapters';
import { parseFeed } from './feed';
import { writeCoverBytes } from '../scanner/covers';
import { unlink } from 'node:fs/promises';
import { isInside, pruneEmptyPodcastDir } from './paths';
import type { DrizzleDb } from '../db';

export class FeedFetchError extends Error {}
export class InvalidFeedError extends Error {}

async function fetchFeedText(feedUrl: string): Promise<string> {
	let response: Response;
	try {
		response = await fetch(feedUrl, { signal: AbortSignal.timeout(20_000) });
	} catch (e: unknown) {
		throw new FeedFetchError(String(e instanceof Error ? e.message : e));
	}
	if (!response.ok) throw new FeedFetchError(`HTTP ${response.status}`);
	return response.text();
}

async function syncEpisodes(
	db: DrizzleDb,
	podcastId: number,
	episodes: Awaited<ReturnType<typeof parseFeed>>['episodes']
): Promise<{ newEpisodes: number; updatedEpisodes: number }> {
	let newEpisodes = 0;
	let updatedEpisodes = 0;
	for (const episode of episodes) {
		const [existing] = await db
			.select({ id: itemsTable.id })
			.from(itemsTable)
			.where(and(eq(itemsTable.parentId, podcastId), eq(itemsTable.guid, episode.guid)));
		let episodeId: number;
		if (existing) {
			await db
				.update(itemsTable)
				.set({
					title: episode.title,
					sortTitle: episode.title.toLowerCase(),
					description: episode.description,
					feedUrl: episode.mediaUrl,
					publishedAt: episode.publishedAt,
					chaptersUrl: episode.chaptersUrl,
					feedDuration: episode.durationSeconds ?? null
				})
				.where(eq(itemsTable.id, existing.id));
			episodeId = existing.id;
			updatedEpisodes += 1;
		} else {
			const [inserted] = await db
				.insert(itemsTable)
				.values({
					kind: 'episode',
					parentId: podcastId,
					title: episode.title,
					sortTitle: episode.title.toLowerCase(),
					description: episode.description,
					feedDuration: episode.durationSeconds ?? null,
					guid: episode.guid,
					feedUrl: episode.mediaUrl,
					publishedAt: episode.publishedAt,
					chaptersUrl: episode.chaptersUrl
				})
				.returning({ id: itemsTable.id });
			episodeId = inserted.id;
			newEpisodes += 1;
		}
		// Podlove chapters ride along in the feed, so they cost nothing to store here.
		// The file's own chapters, read at download time, take precedence over these.
		if (episode.chapters.length > 0) {
			await saveEpisodeChapters(db, episodeId, episode.chapters, episode.durationSeconds ?? 0);
		}
	}
	return { newEpisodes, updatedEpisodes };
}

export async function previewFeed(feedUrl: string) {
	const raw = await fetchFeedText(feedUrl);
	try {
		return await parseFeed(raw);
	} catch (e: unknown) {
		throw new InvalidFeedError(e instanceof Error ? e.message : String(e));
	}
}

const MAX_COVER_BYTES = 5_000_000;

async function storeFeedCover(
	db: DrizzleDb,
	podcastId: number,
	imageUrl: string,
	coversDir: string
): Promise<string | null> {
	try {
		const url = new URL(imageUrl);
		if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;
		const response = await fetch(url, { signal: AbortSignal.timeout(20_000) });
		if (!response.ok) return null;
		const bytes = Buffer.from(await response.arrayBuffer());
		if (bytes.length === 0 || bytes.length > MAX_COVER_BYTES) return null;
		const path = await writeCoverBytes(coversDir, podcastId, bytes);
		await db.update(itemsTable).set({ coverPath: path }).where(eq(itemsTable.id, podcastId));
		return path;
	} catch {
		return null;
	}
}

export async function subscribe(db: DrizzleDb, feedUrl: string, opts: { coversDir?: string } = {}) {
	const raw = await fetchFeedText(feedUrl);
	let parsed: Awaited<ReturnType<typeof parseFeed>>;
	try {
		parsed = await parseFeed(raw);
	} catch (e: unknown) {
		throw new InvalidFeedError(e instanceof Error ? e.message : String(e));
	}
	if (parsed.episodes.length === 0 && !parsed.title)
		throw new InvalidFeedError('Feed enthält weder Titel noch Folgen');

	const [existing] = await db
		.select()
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'podcast'), eq(itemsTable.feedUrl, feedUrl)));
	let podcast = existing;
	if (!podcast) {
		[podcast] = await db
			.insert(itemsTable)
			.values({
				kind: 'podcast',
				title: parsed.title,
				sortTitle: parsed.title.toLowerCase(),
				description: parsed.description,
				feedUrl,
				lastChecked: new Date()
			})
			.returning();
	}
	const { newEpisodes, updatedEpisodes } = await syncEpisodes(db, podcast.id, parsed.episodes);
	await db
		.update(itemsTable)
		.set({ lastChecked: new Date(), description: parsed.description })
		.where(eq(itemsTable.id, podcast.id));
	let coverPath = podcast.coverPath;
	if (opts.coversDir && !coverPath && parsed.imageUrl) {
		coverPath = await storeFeedCover(db, podcast.id, parsed.imageUrl, opts.coversDir);
	}
	return { ...podcast, coverPath, newEpisodes, updatedEpisodes };
}

export async function refresh(db: DrizzleDb, podcastId: number, opts: { coversDir?: string } = {}) {
	const [podcast] = await db
		.select()
		.from(itemsTable)
		.where(and(eq(itemsTable.id, podcastId), eq(itemsTable.kind, 'podcast')));
	if (!podcast) throw new Error('not found');
	const raw = await fetchFeedText(podcast.feedUrl!);
	let parsed: Awaited<ReturnType<typeof parseFeed>>;
	try {
		parsed = await parseFeed(raw);
	} catch (e: unknown) {
		throw new InvalidFeedError(e instanceof Error ? e.message : String(e));
	}
	const { newEpisodes, updatedEpisodes } = await syncEpisodes(db, podcast.id, parsed.episodes);
	await db
		.update(itemsTable)
		.set({ lastChecked: new Date(), description: parsed.description })
		.where(eq(itemsTable.id, podcast.id));
	let coverPath = podcast.coverPath;
	if (opts.coversDir && !coverPath && parsed.imageUrl) {
		coverPath = await storeFeedCover(db, podcast.id, parsed.imageUrl, opts.coversDir);
	}
	return { ...podcast, coverPath, newEpisodes, updatedEpisodes };
}

export async function unsubscribe(db: DrizzleDb, podcastId: number, podcastsDir: string) {
	const episodeRows = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(and(eq(itemsTable.parentId, podcastId), eq(itemsTable.kind, 'episode')));

	const trackedPaths = await db
		.select({ path: tracksTable.path })
		.from(tracksTable)
		.innerJoin(itemsTable, eq(itemsTable.id, tracksTable.itemId))
		.where(and(eq(itemsTable.parentId, podcastId), isNotNull(tracksTable.path)));

	const [deleted] = await db
		.delete(itemsTable)
		.where(and(eq(itemsTable.id, podcastId), eq(itemsTable.kind, 'podcast')))
		.returning({ id: itemsTable.id });
	if (!deleted) throw new Error('not found');

	let filesDeleted = 0;
	let filesKept = 0;
	for (const { path } of trackedPaths) {
		if (!isInside(path, podcastsDir)) {
			filesKept += 1;
			continue;
		}
		try {
			await unlink(path);
			filesDeleted += 1;
		} catch {
			filesKept += 1;
		}
		await pruneEmptyPodcastDir(path, podcastsDir);
	}
	return { episodes: episodeRows.length, filesDeleted, filesKept };
}
