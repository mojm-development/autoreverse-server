import { readdir, stat } from 'node:fs/promises';
import { join, extname, basename, relative, sep } from 'node:path';
import { readTags, type TrackTags } from './tags';
import { readChapters, type Chapter } from './chapters';
import type { ProgressFn } from '../admin/scanState';

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

/** One folder that could not be scanned or stored, with the reason. Collected
 * instead of thrown: a single unreadable album must not cost the operator the
 * other several thousand. */
export interface ScanFailure {
	path: string;
	message: string;
}

function reason(e: unknown): string {
	const code = (e as NodeJS.ErrnoException)?.code;
	if (code === 'ENOENT') return 'Verzeichnis existiert nicht';
	if (code === 'EACCES' || code === 'EPERM') return 'Verzeichnis ist nicht lesbar';
	if (code) return `Verzeichnis nicht zugänglich (${code})`;
	return e instanceof Error ? e.message : String(e);
}

/** Why a library root cannot be scanned, or null if it can be.
 *
 * allDirectories and audioFilesIn both swallow readdir failures so that one
 * bad subfolder doesn't abort its siblings. Applied to the root itself that
 * turns "the mount is gone" into "the library is empty" — indistinguishable
 * from a genuinely empty library, and the caller must tell them apart before
 * markMissing empties the database on the strength of it. */
export async function libraryRootProblem(root: string): Promise<string | null> {
	let stats;
	try {
		stats = await stat(root);
	} catch (e) {
		return reason(e);
	}
	if (!stats.isDirectory()) return 'Pfad ist kein Verzeichnis';
	try {
		await readdir(root);
	} catch (e) {
		return reason(e);
	}
	return null;
}

export interface RawTrack {
	path: string;
	tags: TrackTags;
	mtime: number;
	size: number;
}

/** Orders a folder's files and numbers them 1..n.
 *
 * The number stored is the folder's own ordering, NOT the file's track tag.
 * Track tags repeat constantly — loose singles each tagged track 1, a
 * two-disc album living in one folder, a folder mixing tagged and untagged
 * files — and UNIQUE(item_id, position) turns every repeat into a failed
 * insert that aborts the whole scan pass. Ordering still follows disc then
 * track, so the sequence a listener sees is unchanged; only the number
 * behind it is ours. Nothing renders it: queries.ts uses position purely
 * to ORDER BY. */
export function orderTracks(raw: RawTrack[], fileOrder: string[]): ScannedTrack[] {
	const fallback = (path: string) => fileOrder.indexOf(path) + 1;
	const sorted = [...raw].sort(
		(a, b) =>
			(a.tags.disc ?? 0) - (b.tags.disc ?? 0) ||
			(a.tags.track ?? fallback(a.path)) - (b.tags.track ?? fallback(b.path)) ||
			fallback(a.path) - fallback(b.path)
	);
	return sorted.map((t, i) => ({
		path: t.path,
		position: i + 1,
		title: t.tags.title ?? basename(t.path, extname(t.path)),
		disc: t.tags.disc,
		duration: t.tags.duration,
		mtime: t.mtime,
		size: t.size
	}));
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
	const tracks = orderTracks(rawTracks, files);

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

/** Walks root and scans every folder in it, isolating failures per folder.
 * scanFolder touches the filesystem and the tag reader at every step, so a
 * single unreadable file used to abort the entire pass — the caller got an
 * exception and no items at all, however many folders had already scanned
 * cleanly. */
export async function scanTree(
	root: string,
	kind: 'book' | 'album',
	known: Record<string, [number, number]>,
	failures: ScanFailure[] = [],
	onProgress?: ProgressFn
): Promise<ScannedItem[]> {
	const dirs = await allDirectories(root);
	const results: ScannedItem[] = [];
	for (const [i, dir] of dirs.entries()) {
		try {
			const scanned = await scanFolder(root, dir, kind, known);
			if (scanned) results.push(scanned);
		} catch (e) {
			failures.push({ path: dir, message: reason(e) });
		}
		onProgress?.(i + 1, dirs.length);
	}
	return results;
}

export async function scanBooks(
	root: string,
	known: Record<string, [number, number]>,
	failures: ScanFailure[] = [],
	onProgress?: ProgressFn
): Promise<ScannedItem[]> {
	return scanTree(root, 'book', known, failures, onProgress);
}
