import { join, extname, dirname, relative, resolve } from 'node:path';
import { access, rmdir } from 'node:fs/promises';
import { eq } from 'drizzle-orm';
import { tracks as tracksTable } from '../db/schema';
import type { DrizzleDb } from '../db';

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

export function isInside(path: string, root: string): boolean {
	const rel = relative(resolve(root), resolve(path));
	return !rel.startsWith('..');
}

export function suffixFor(mediaUrl: string): string {
	return extname(new URL(mediaUrl).pathname) || '.mp3';
}

export async function fileExists(path: string): Promise<boolean> {
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
export async function episodeDestination(
	db: DrizzleDb,
	podcastsDir: string,
	episode: { id: number; title: string },
	podcastTitle: string,
	suffix: string
): Promise<string> {
	const folder = join(podcastsDir, safeFileName(podcastTitle, 'Podcast'));
	const base = safeFileName(episode.title, String(episode.id));
	const candidate = join(folder, `${base}${suffix}`);
	const withId = join(folder, `${base} (${episode.id})${suffix}`);
	const [claimed] = await db
		.select({ itemId: tracksTable.itemId })
		.from(tracksTable)
		.where(eq(tracksTable.path, candidate));
	if (claimed) return claimed.itemId === episode.id ? candidate : withId;
	if (!(await fileExists(candidate))) return candidate;
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
