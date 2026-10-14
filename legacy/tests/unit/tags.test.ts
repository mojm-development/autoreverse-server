import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readTags } from '../../src/lib/server/scanner/tags';

function makeMp3(path: string, tags: Record<string, string>) {
	const args = ['-y', '-f', 'lavfi', '-i', 'anullsrc=r=8000:cl=mono', '-t', '1'];
	for (const [k, v] of Object.entries(tags)) args.push('-metadata', `${k}=${v}`);
	args.push(path);
	execFileSync('ffmpeg', args, { stdio: 'ignore' });
}

describe('readTags', () => {
	it('reads title/artist/album/track/year from an mp3', () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-tags-'));
		const path = join(dir, 'a.mp3');
		makeMp3(path, {
			title: 'Erste Bahn',
			artist: 'Ansa Volt',
			album: 'Nordlicht',
			track: '1',
			date: '2022'
		});
		const tags = readTags(path);
		return tags.then((t) => {
			expect(t.title).toBe('Erste Bahn');
			expect(t.artist).toBe('Ansa Volt');
			expect(t.album).toBe('Nordlicht');
			expect(t.track).toBe(1);
			expect(t.year).toBe(2022);
			expect(t.readable).toBe(true);
			expect(t.duration).toBeGreaterThan(0);
		});
	});

	it('returns an all-null unreadable result for a non-audio file, never throws', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-tags-'));
		const path = join(dir, 'not-audio.mp3');
		writeFileSync(path, 'not actually audio');
		const tags = await readTags(path);
		expect(tags.readable).toBe(false);
		expect(tags.title).toBeNull();
		expect(tags.duration).toBe(0);
	});
});
