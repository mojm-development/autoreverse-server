import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, statSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { scanBooks } from '../../src/lib/server/scanner/books';
import { scanMusic } from '../../src/lib/server/scanner/music';

function makeAudio(path: string, tags: Record<string, string> = {}) {
	const args = ['-y', '-f', 'lavfi', '-i', 'anullsrc=r=8000:cl=mono', '-t', '1'];
	for (const [k, v] of Object.entries(tags)) args.push('-metadata', `${k}=${v}`);
	args.push(path);
	execFileSync('ffmpeg', args, { stdio: 'ignore' });
}

describe('scanBooks', () => {
	it('derives author/series from Author/Series/Book folder depth', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		const bookDir = join(root, 'Andreas Eschbach', 'Solarstation', 'Buch 1');
		mkdirSync(bookDir, { recursive: true });
		makeAudio(join(bookDir, '01.mp3'), { title: 'Teil 1' });
		makeAudio(join(bookDir, '02.mp3'), { title: 'Teil 2' });

		const [scanned] = await scanBooks(root, {});
		expect(scanned.author).toBe('Andreas Eschbach');
		expect(scanned.series).toBe('Solarstation');
		expect(scanned.tracks).toHaveLength(2);
		expect(scanned.unchanged).toBe(false);
	});

	it('synthesizes one chapter per track when there is more than one track file', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		const bookDir = join(root, 'A', 'B');
		mkdirSync(bookDir, { recursive: true });
		makeAudio(join(bookDir, '01.mp3'), { title: 'Erster Teil' });
		makeAudio(join(bookDir, '02.mp3'), { title: 'Zweiter Teil' });

		const [scanned] = await scanBooks(root, {});
		expect(scanned.chapters).toHaveLength(2);
		expect(scanned.chapters[0].start).toBe(0);
		expect(scanned.chapters[1].start).toBeCloseTo(scanned.tracks[0].duration, 5);
	});

	it('marks a folder unchanged when the known {mtime,size} map matches exactly', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		const bookDir = join(root, 'A', 'B');
		mkdirSync(bookDir, { recursive: true });
		const filePath = join(bookDir, '01.mp3');
		makeAudio(filePath);
		const stats = statSync(filePath);

		const [scanned] = await scanBooks(root, { [filePath]: [stats.mtimeMs / 1000, stats.size] });
		expect(scanned.unchanged).toBe(true);
		expect(scanned.tracks).toEqual([]);
	});

	it('skips a directory with zero audio files as an item, but still descends into it', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		mkdirSync(join(root, 'Empty'), { recursive: true });
		const bookDir = join(root, 'Empty', 'HasAudio');
		mkdirSync(bookDir, { recursive: true });
		makeAudio(join(bookDir, '01.mp3'));
		const scanned = await scanBooks(root, {});
		expect(scanned).toHaveLength(1);
		expect(scanned[0].sourcePath).toBe(bookDir);
	});

	it('detects when a file is removed from disk (regression: removed-file detection)', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		const bookDir = join(root, 'A', 'B');
		mkdirSync(bookDir, { recursive: true });
		const file1 = join(bookDir, '01.mp3');
		const file2 = join(bookDir, '02.mp3');
		makeAudio(file1);
		makeAudio(file2);

		// First scan: both files present and known
		const stats1 = statSync(file1);
		const stats2 = statSync(file2);
		const known = {
			[file1]: [stats1.mtimeMs / 1000, stats1.size],
			[file2]: [stats2.mtimeMs / 1000, stats2.size]
		} as Record<string, [number, number]>;

		// Second scan: delete file2, then scan
		rmSync(file2);
		const [scanned] = await scanBooks(root, known);
		expect(scanned.unchanged).toBe(false); // MUST detect change, not silently drop track
		expect(scanned.tracks).toHaveLength(1);
		expect(scanned.tracks[0].path).toBe(file1);
	});

	it('does not false-positive on sibling folders when scanning with global known map', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		const bookDir1 = join(root, 'A1', 'B1');
		const bookDir2 = join(root, 'A2', 'B2');
		mkdirSync(bookDir1, { recursive: true });
		mkdirSync(bookDir2, { recursive: true });

		const file1a = join(bookDir1, '01.mp3');
		const file1b = join(bookDir1, '02.mp3');
		const file2a = join(bookDir2, '01.mp3');
		const file2b = join(bookDir2, '02.mp3');

		makeAudio(file1a);
		makeAudio(file1b);
		makeAudio(file2a);
		makeAudio(file2b);

		// Build known map with files from BOTH folders
		const stats1a = statSync(file1a);
		const stats1b = statSync(file1b);
		const stats2a = statSync(file2a);
		const stats2b = statSync(file2b);
		const known = {
			[file1a]: [stats1a.mtimeMs / 1000, stats1a.size],
			[file1b]: [stats1b.mtimeMs / 1000, stats1b.size],
			[file2a]: [stats2a.mtimeMs / 1000, stats2a.size],
			[file2b]: [stats2b.mtimeMs / 1000, stats2b.size]
		} as Record<string, [number, number]>;

		// Scan only bookDir1 with the global known map
		// It should correctly mark as unchanged (direct-children-only filtering)
		// even though bookDir2's files are in the known map
		const scanned = await scanBooks(bookDir1, known);
		expect(scanned).toHaveLength(1);
		expect(scanned[0].unchanged).toBe(true);
		expect(scanned[0].sourcePath).toBe(bookDir1);
		expect(scanned[0].tracks).toEqual([]);
	});
});

describe('scanMusic', () => {
	it('never calls readChapters — chapters is always []', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-music-'));
		const albumDir = join(root, 'Ansa Volt', 'Nordlicht');
		mkdirSync(albumDir, { recursive: true });
		makeAudio(join(albumDir, '01.mp3'), { album: 'Nordlicht', artist: 'Ansa Volt' });
		const [scanned] = await scanMusic(root, {});
		expect(scanned.chapters).toEqual([]);
	});

	it('artist fallback chain: albumartist tag > artist tag of first track > folder name', async () => {
		const root = mkdtempSync(join(tmpdir(), 'autoreverse-music-'));
		const albumDir = join(root, 'Folder Artist', 'Album');
		mkdirSync(albumDir, { recursive: true });
		makeAudio(join(albumDir, '01.mp3'), { artist: 'Track Artist' }); // no albumartist tag
		const [scanned] = await scanMusic(root, {});
		expect(scanned.artist).toBe('Track Artist');
	});
});
