import { describe, it, expect } from 'vitest';
import { parseRange, RangeNotSatisfiable, mediaType } from '../../src/lib/server/streaming/ranges';

describe('parseRange', () => {
	const SIZE = 1000;

	it('returns null for no header', () => {
		expect(parseRange(null, SIZE)).toBeNull();
	});

	it('returns null for an unparseable header (caller falls back to whole-file 200)', () => {
		expect(parseRange('bytes = 0-99', SIZE)).toBeNull();
		expect(parseRange('nonsense', SIZE)).toBeNull();
	});

	it('parses a normal range', () => {
		expect(parseRange('bytes=0-99', SIZE)).toEqual({ start: 0, end: 99 });
	});

	it('caps an open-ended range to size-1', () => {
		expect(parseRange('bytes=500-', SIZE)).toEqual({ start: 500, end: 999 });
	});

	it('caps an end beyond size to size-1', () => {
		expect(parseRange('bytes=0-99999', SIZE)).toEqual({ start: 0, end: 999 });
	});

	it('parses a suffix range ("last N bytes")', () => {
		expect(parseRange('bytes=-500', SIZE)).toEqual({ start: 500, end: 999 });
	});

	it('throws RangeNotSatisfiable for a start beyond size', () => {
		expect(() => parseRange('bytes=1000-', SIZE)).toThrow(RangeNotSatisfiable);
	});

	it('throws RangeNotSatisfiable for end before start', () => {
		expect(() => parseRange('bytes=5-2', SIZE)).toThrow(RangeNotSatisfiable);
	});

	it('throws RangeNotSatisfiable for a non-positive suffix length', () => {
		expect(() => parseRange('bytes=-0', SIZE)).toThrow(RangeNotSatisfiable);
	});

	it('ignores everything after a comma (single range only)', () => {
		expect(parseRange('bytes=0-99,200-299', SIZE)).toEqual({ start: 0, end: 99 });
	});
});

describe('mediaType', () => {
	it('maps known extensions, defaults to octet-stream', () => {
		expect(mediaType('/x/a.mp3')).toBe('audio/mpeg');
		expect(mediaType('/x/a.m4b')).toBe('audio/mp4');
		expect(mediaType('/x/a.flac')).toBe('audio/flac');
		expect(mediaType('/x/a.xyz')).toBe('application/octet-stream');
	});
});
