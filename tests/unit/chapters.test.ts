import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readChapters } from '../../src/lib/server/scanner/chapters';

describe('readChapters', () => {
	it('returns [] for a file with no chapter atoms', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'capstan-chapters-'));
		const path = join(dir, 'a.mp3');
		execFileSync(
			'ffmpeg',
			['-y', '-f', 'lavfi', '-i', 'anullsrc=r=8000:cl=mono', '-t', '1', path],
			{ stdio: 'ignore' }
		);
		expect(await readChapters(path)).toEqual([]);
	});

	it('returns [] (never throws) when ffprobe is missing or the file does not exist', async () => {
		expect(await readChapters('/nonexistent/file.m4b')).toEqual([]);
	});
});
