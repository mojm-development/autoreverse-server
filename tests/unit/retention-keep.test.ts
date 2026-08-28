import { describe, it, expect } from 'vitest';
import { clampKeep, KEEP_MAX, KEEP_MIN } from '../../src/lib/server/podcasts/retention';

describe('clampKeep', () => {
	it('keeps a sensible number as it is', () => {
		expect(clampKeep(3)).toBe(3);
		expect(clampKeep(KEEP_MIN)).toBe(KEEP_MIN);
		expect(clampKeep(KEEP_MAX)).toBe(KEEP_MAX);
	});

	it('pulls anything outside the range back in', () => {
		expect(clampKeep(-5)).toBe(KEEP_MIN);
		expect(clampKeep(999)).toBe(KEEP_MAX);
	});

	it('refuses to make a fraction of an episode', () => {
		expect(clampKeep(2.9)).toBe(2);
	});
});
