import { describe, it, expect } from 'vitest';
import { normalizeDir } from '../../src/lib/server/settings/libraryPaths';

describe('normalizeDir', () => {
	it('strips trailing slashes so the scanner prefix matches stored source paths', () => {
		expect(normalizeDir('/media/musik/')).toBe('/media/musik');
		expect(normalizeDir('/media/musik///')).toBe('/media/musik');
		expect(normalizeDir('  /media/musik/  ')).toBe('/media/musik');
	});

	it('leaves a plain path and the filesystem root alone', () => {
		expect(normalizeDir('/media/musik')).toBe('/media/musik');
		expect(normalizeDir('/')).toBe('/');
		expect(normalizeDir('')).toBe('');
	});
});
