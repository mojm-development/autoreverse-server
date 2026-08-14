import { join, extname } from 'node:path';
import { rename, unlink, mkdir, writeFile } from 'node:fs/promises';
import { readTags } from '../scanner/tags';
import { eq } from 'drizzle-orm';
import { items as itemsTable, tracks as tracksTable } from '../db/schema';
import type { DrizzleDb } from '../db';

export class EpisodeNotDownloadableError extends Error {}
export class EpisodeFetchError extends Error {}
export class EpisodeStorageError extends Error {}

function destinationFor(podcastsDir: string, episodeId: number, mediaUrl: string): string {
	const suffix = extname(new URL(mediaUrl).pathname) || '.mp3';
	return join(podcastsDir, `${episodeId}${suffix}`);
}

export async function downloadEpisode(
	db: DrizzleDb,
	episodeId: number,
	podcastsDir: string
): Promise<{ trackId: number; duration: number }> {
	const [episode] = await db.select().from(itemsTable).where(eq(itemsTable.id, episodeId));
	if (!episode) throw new Error('not found');
	const mediaUrl = episode.feedUrl; // repurposed field on episode rows
	if (!mediaUrl) throw new EpisodeNotDownloadableError('Keine Audiodatei in diesem Feed-Eintrag');

	await mkdir(podcastsDir, { recursive: true });
	const destination = destinationFor(podcastsDir, episodeId, mediaUrl);
	const temp = `${destination}.part`;

	let response: Response;
	try {
		response = await fetch(mediaUrl, { redirect: 'follow', signal: AbortSignal.timeout(20_000) });
		if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
	} catch (e: unknown) {
		throw new EpisodeFetchError(String(e instanceof Error ? e.message : e));
	}

	try {
		await writeFile(temp, Buffer.from(await response.arrayBuffer()));
		await rename(temp, destination);
	} catch (e: unknown) {
		await unlink(temp).catch(() => {});
		throw new EpisodeStorageError(String(e instanceof Error ? e.message : e));
	}

	const tags = await readTags(destination);
	const [track] = await db
		.insert(tracksTable)
		.values({
			itemId: episodeId,
			position: 1,
			path: destination,
			duration: tags.duration,
			title: episode.title
		})
		.onConflictDoUpdate({ target: tracksTable.path, set: { duration: tags.duration } })
		.returning({ id: tracksTable.id });
	return { trackId: track.id, duration: tags.duration };
}
