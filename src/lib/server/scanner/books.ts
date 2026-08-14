import { readdir, stat } from 'node:fs/promises';
import { join, extname, basename, relative, sep } from 'node:path';
import { readTags } from './tags';
import { readChapters, type Chapter } from './chapters';

export const AUDIO = new Set(['.mp3', '.m4a', '.m4b', '.flac', '.ogg', '.opus', '.wav', '.aac']);

export interface ScannedTrack {
	path: string;
	position: number;
	title: string | null;
	disc: number | null;
	duration: number;
	mtime: number;
	size: number;
}

export interface ScannedItem {
	sourcePath: string;
	kind: 'book' | 'album';
	title: string;
	author?: string | null;
	artist?: string | null;
	albumArtist?: string | null;
	series?: string | null;
	year?: number | null;
	seriesIndex: null; // never derived — matches the Python scanner exactly (DB column exists, never written)
	tracks: ScannedTrack[];
	chapters: Chapter[];
	unchanged: boolean;
}

/** Enumerates every directory under root (including root itself), using a
 * manual recursive readdir (Node's fs has no os.walk equivalent) — matches
 * the Python scanner's deliberate choice of os.walk over glob/rglob for
 * permission fidelity; Node's readdir/stat pairing has the same property
 * (a stat failure on one entry doesn't abort the sibling scan). */
export async function allDirectories(root: string): Promise<string[]> {
	const found: string[] = [root];
	async function walk(dir: string) {
		let entries;
		try {
			entries = await readdir(dir, { withFileTypes: true });
		} catch {
			return;
		}
		for (const entry of entries) {
			if (entry.isDirectory()) {
				const full = join(dir, entry.name);
				found.push(full);
				await walk(full);
			}
		}
	}
	await walk(root);
	return found;
}

async function audioFilesIn(dir: string): Promise<string[]> {
	let entries;
	try {
		entries = await readdir(dir, { withFileTypes: true });
	} catch {
		return [];
	}
	return entries
		.filter((e) => e.isFile() && AUDIO.has(extname(e.name).toLowerCase()))
		.map((e) => join(dir, e.name))
		.sort();
}

export async function scanFolder(
	root: string,
	dir: string,
	kind: 'book' | 'album',
	known: Record<string, [number, number]>
): Promise<ScannedItem | null> {
	const files = await audioFilesIn(dir);
	if (files.length === 0) return null;

	const stats = new Map<string, [number, number]>();
	for (const file of files) {
		const s = await stat(file);
		stats.set(file, [s.mtimeMs / 1000, s.size]);
	}
	const knownForFolder = files.every((f) => f in known);
	const dirPrefix = `${dir}${sep}`;
	const knownPathsInFolder = Object.keys(known).filter((k) => {
		if (!k.startsWith(dirPrefix)) return false;
		return !k.slice(dirPrefix.length).includes(sep); // direct child only, matches audioFilesIn's own non-recursive semantics
	});
	const unchanged =
		knownForFolder &&
		files.every((f) => {
			const [mtime, size] = known[f];
			const [actualMtime, actualSize] = stats.get(f)!;
			return mtime === actualMtime && size === actualSize;
		}) &&
		knownPathsInFolder.length === files.length;

	const parts = relative(root, dir).split(sep).filter(Boolean);
	const folderAuthorOrArtist = parts.length >= 2 ? parts[0] : null;
	const series = kind === 'book' && parts.length >= 3 ? parts[1] : null;

	if (unchanged) {
		return {
			sourcePath: dir,
			kind,
			title: basename(dir),
			author: kind === 'book' ? folderAuthorOrArtist : undefined,
			artist: kind === 'album' ? folderAuthorOrArtist : undefined,
			series,
			seriesIndex: null,
			tracks: [],
			chapters: [],
			unchanged: true
		};
	}

	const tagsByFile = await Promise.all(files.map((f) => readTags(f)));
	const rawTracks = files.map((path, i) => ({
		path,
		tags: tagsByFile[i],
		mtime: stats.get(path)![0],
		size: stats.get(path)![1]
	}));
	rawTracks.sort(
		(a, b) =>
			(a.tags.disc ?? 0) - (b.tags.disc ?? 0) ||
			(a.tags.track ?? files.indexOf(a.path) + 1) - (b.tags.track ?? files.indexOf(b.path) + 1)
	);

	const tracks: ScannedTrack[] = rawTracks.map((t, i) => ({
		path: t.path,
		position: t.tags.track ?? i + 1,
		title: t.tags.title ?? basename(t.path, extname(t.path)),
		disc: t.tags.disc,
		duration: t.tags.duration,
		mtime: t.mtime,
		size: t.size
	}));

	const firstTags = rawTracks[0].tags;
	const title = firstTags.album || basename(dir);
	const author = kind === 'book' ? firstTags.artist || folderAuthorOrArtist : undefined;
	const artist =
		kind === 'album'
			? firstTags.albumArtist || firstTags.artist || folderAuthorOrArtist
			: undefined;
	const albumArtist = kind === 'album' ? firstTags.albumArtist : undefined;
	const year = kind === 'album' ? firstTags.year : undefined;

	let chapters: Chapter[] = [];
	if (kind === 'book') {
		chapters = tracks.length === 1 ? await readChapters(tracks[0].path) : [];
		if (chapters.length === 0) {
			let elapsed = 0;
			chapters = tracks.map((t) => {
				const chapter = {
					title: t.title || `Teil ${t.position}`,
					start: elapsed,
					end: elapsed + t.duration
				};
				elapsed += t.duration;
				return chapter;
			});
		}
	}

	return {
		sourcePath: dir,
		kind,
		title,
		author,
		artist,
		albumArtist,
		series,
		year,
		seriesIndex: null,
		tracks,
		chapters,
		unchanged: false
	};
}

export async function scanBooks(
	root: string,
	known: Record<string, [number, number]>
): Promise<ScannedItem[]> {
	const dirs = await allDirectories(root);
	const results: ScannedItem[] = [];
	for (const dir of dirs) {
		const scanned = await scanFolder(root, dir, 'book', known);
		if (scanned) results.push(scanned);
	}
	return results;
}
