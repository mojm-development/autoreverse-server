import { describe, it, expect } from 'vitest';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { findCoverFile } from '../../src/lib/server/scanner/covers';

describe('findCoverFile', () => {
	it('finds cover.jpg before folder.jpg (name-then-suffix priority order)', () => {
		const dir = mkdtempSync(join(tmpdir(), 'capstan-covers-'));
		writeFileSync(join(dir, 'folder.jpg'), '');
		writeFileSync(join(dir, 'cover.png'), '');
		return findCoverFile(dir).then((found) => expect(found).toBe(join(dir, 'cover.png')));
	});

	it('returns null when no candidate file exists', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'capstan-covers-'));
		expect(await findCoverFile(dir)).toBeNull();
	});

	it('ignores directories named like cover files (cover.jpg dir should return null)', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'capstan-covers-'));
		mkdirSync(join(dir, 'cover.jpg'));
		expect(await findCoverFile(dir)).toBeNull();
	});

	it('skips directory and returns next valid candidate file', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'capstan-covers-'));
		mkdirSync(join(dir, 'cover.jpg'));
		writeFileSync(join(dir, 'folder.png'), '');
		expect(await findCoverFile(dir)).toBe(join(dir, 'folder.png'));
	});
});
