import { join, extname, dirname, relative, resolve } from 'node:path';
import { rename, unlink, mkdir, writeFile, access, rmdir } from 'node:fs/promises';
import { readTags } from '../scanner/tags';
import { and, eq } from 'drizzle-orm';
import { items as itemsTable, tracks as tracksTable } from '../db/schema';
import type { DrizzleDb } from '../db';

export class EpisodeNotDownloadableError extends Error {}
export class EpisodeFetchError extends Error {}
export class EpisodeStorageError extends Error {}

const MAX_NAME_LENGTH = 120;

// Titles come straight from a feed, so they may contain anything: path separators, characters that
// are illegal on Windows/exFAT shares, control codes, or nothing usable at all. Everything outside
// the safe set collapses into a single space; trailing dots and spaces go because Windows strips
// them silently, which would leave the stored path and the file on disk disagreeing.
export function safeFileName(raw: string, fallback: string): string {
	const cleaned = raw
		// eslint-disable-next-line no-control-regex
		.replace(/[\u0000-\u001f<>:"/\\|?*]/g, ' ')
		.replace(/\s+/g, ' ')
		.trim()
		.slice(0, MAX_NAME_LENGTH)
		.replace(/[. ]+$/, '');
	return cleaned || fallback;
}

async function exists(path: string): Promise<boolean> {
	try {
		await access(path);
		return true;
	} catch {
		return false;
	}
}

// <podcastsDir>/<Podcast>/<Folge>.mp3 — two episodes of one podcast may well share a title, so a
// name already taken by another file gets the episode id appended instead of overwriting it. The
// episode's own file is not in the way: re-downloading an episode replaces it in place.
async function destinationFor(
	db: DrizzleDb,
	podcastsDir: string,
	episode: { id: number; title: string },
	podcastTitle: string,
	mediaUrl: string
): Promise<string> {
	const suffix = extname(new URL(mediaUrl).pathname) || '.mp3';
	const folder = join(podcastsDir, safeFileName(podcastTitle, 'Podcast'));
	const base = safeFileName(episode.title, String(episode.id));
	const candidate = join(folder, `${base}${suffix}`);
	const withId = join(folder, `${base} (${episode.id})${suffix}`);
	const [claimed] = await db
		.select({ itemId: tracksTable.itemId })
		.from(tracksTable)
		.where(eq(tracksTable.path, candidate));
	if (claimed) return claimed.itemId === episode.id ? candidate : withId;
	if (!(await exists(candidate))) return candidate;
	return withId;
}

// Deleting the last episode of a podcast leaves its folder behind. Drop it once it is empty and
// still below the podcasts root — rmdir refuses a folder that anything else still lives in.
export async function pruneEmptyPodcastDir(path: string, podcastsDir: string): Promise<void> {
	const dir = resolve(dirname(path));
	const root = resolve(podcastsDir);
	if (dir === root || relative(root, dir).startsWith('..')) return;
	await rmdir(dir).catch(() => undefined);
}

export async function downloadEpisode(
	db: DrizzleDb,
	episodeId: number,
	podcastsDir: string
): Promise<{ trackId: number; duration: number }> {
	const [episode] = await db
		.select()
		.from(itemsTable)
		.where(and(eq(itemsTable.id, episodeId), eq(itemsTable.kind, 'episode')));
	if (!episode) throw new Error('not found');
	const mediaUrl = episode.feedUrl;
	if (!mediaUrl) throw new EpisodeNotDownloadableError('Keine Audiodatei in diesem Feed-Eintrag');

	const [podcast] = episode.parentId
		? await db
				.select({ title: itemsTable.title })
				.from(itemsTable)
				.where(eq(itemsTable.id, episode.parentId))
		: [];
	const destination = await destinationFor(
		db,
		podcastsDir,
		episode,
		podcast?.title ?? 'Podcast',
		mediaUrl
	);
	const temp = `${destination}.part`;

	let response: Response;
	try {
		response = await fetch(mediaUrl, { redirect: 'follow', signal: AbortSignal.timeout(20_000) });
		if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
	} catch (e: unknown) {
		throw new EpisodeFetchError(String(e instanceof Error ? e.message : e));
	}

	try {
		await mkdir(dirname(destination), { recursive: true });
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
