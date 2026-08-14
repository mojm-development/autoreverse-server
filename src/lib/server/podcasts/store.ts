import { and, eq, isNotNull } from 'drizzle-orm';
import { items as itemsTable, tracks as tracksTable } from '../db/schema';
import { parseFeed } from './feed';
import { relative, resolve } from 'node:path';
import { unlink } from 'node:fs/promises';
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
) {
	for (const episode of episodes) {
		const [existing] = await db
			.select({ id: itemsTable.id })
			.from(itemsTable)
			.where(and(eq(itemsTable.parentId, podcastId), eq(itemsTable.guid, episode.guid)));
		if (existing) {
			await db
				.update(itemsTable)
				.set({
					title: episode.title,
					sortTitle: episode.title.toLowerCase(),
					feedUrl: episode.mediaUrl,
					publishedAt: episode.publishedAt
				})
				.where(eq(itemsTable.id, existing.id));
		} else {
			await db.insert(itemsTable).values({
				kind: 'episode',
				parentId: podcastId,
				title: episode.title,
				sortTitle: episode.title.toLowerCase(),
				guid: episode.guid,
				feedUrl: episode.mediaUrl, // repurposed on episode rows to hold the media/enclosure URL, not a feed address
				publishedAt: episode.publishedAt
			});
		}
	}
}

export async function subscribe(db: DrizzleDb, feedUrl: string) {
	const raw = await fetchFeedText(feedUrl);
	const parsed = await parseFeed(raw);
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
				feedUrl,
				lastChecked: new Date()
			})
			.returning();
	}
	await syncEpisodes(db, podcast.id, parsed.episodes);
	await db.update(itemsTable).set({ lastChecked: new Date() }).where(eq(itemsTable.id, podcast.id));
	return podcast;
}

export async function refresh(db: DrizzleDb, podcastId: number) {
	const [podcast] = await db.select().from(itemsTable).where(eq(itemsTable.id, podcastId));
	if (!podcast) throw new Error('not found');
	const raw = await fetchFeedText(podcast.feedUrl!);
	const parsed = await parseFeed(raw);
	await syncEpisodes(db, podcast.id, parsed.episodes);
	await db.update(itemsTable).set({ lastChecked: new Date() }).where(eq(itemsTable.id, podcast.id));
	return podcast;
}

function isInside(path: string, root: string): boolean {
	const rel = relative(resolve(root), resolve(path));
	return !rel.startsWith('..');
}

export async function unsubscribe(db: DrizzleDb, podcastId: number, podcastsDir: string) {
	// Episode count is independent of download state — most episodes never get downloaded,
	// so this can't be derived from trackedPaths below (which only covers downloaded ones).
	const episodeRows = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(and(eq(itemsTable.parentId, podcastId), eq(itemsTable.kind, 'episode')));

	// Capture track paths BEFORE the delete — nothing to read them from after.
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
			filesKept += 1; // swallowed — the subscription is gone regardless of file cleanup
		}
	}
	return { episodes: episodeRows.length, filesDeleted, filesKept };
}
