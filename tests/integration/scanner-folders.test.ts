import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, statSync } from 'node:fs';
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
		const root = mkdtempSync(join(tmpdir(), 'capstan-books-'));
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
		const root = mkdtempSync(join(tmpdir(), 'capstan-books-'));
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
		const root = mkdtempSync(join(tmpdir(), 'capstan-books-'));
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
		const root = mkdtempSync(join(tmpdir(), 'capstan-books-'));
		mkdirSync(join(root, 'Empty'), { recursive: true });
		const bookDir = join(root, 'Empty', 'HasAudio');
		mkdirSync(bookDir, { recursive: true });
		makeAudio(join(bookDir, '01.mp3'));
		const scanned = await scanBooks(root, {});
		expect(scanned).toHaveLength(1);
		expect(scanned[0].sourcePath).toBe(bookDir);
	});
});

describe('scanMusic', () => {
	it('never calls readChapters — chapters is always []', async () => {
		const root = mkdtempSync(join(tmpdir(), 'capstan-music-'));
		const albumDir = join(root, 'Ansa Volt', 'Nordlicht');
		mkdirSync(albumDir, { recursive: true });
		makeAudio(join(albumDir, '01.mp3'), { album: 'Nordlicht', artist: 'Ansa Volt' });
		const [scanned] = await scanMusic(root, {});
		expect(scanned.chapters).toEqual([]);
	});

	it('artist fallback chain: albumartist tag > artist tag of first track > folder name', async () => {
		const root = mkdtempSync(join(tmpdir(), 'capstan-music-'));
		const albumDir = join(root, 'Folder Artist', 'Album');
		mkdirSync(albumDir, { recursive: true });
		makeAudio(join(albumDir, '01.mp3'), { artist: 'Track Artist' }); // no albumartist tag
		const [scanned] = await scanMusic(root, {});
		expect(scanned.artist).toBe('Track Artist');
	});
});
