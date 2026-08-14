import { describe, it, expect, beforeAll } from 'vitest';
import {
	parseRange,
	RangeNotSatisfiable,
	mediaType,
	rangeResponse
} from '../../src/lib/server/streaming/ranges';
import { writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

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

	it('maps .m4a to audio/mp4', () => {
		expect(mediaType('/x/a.m4a')).toBe('audio/mp4');
	});

	it('maps .ogg to audio/ogg', () => {
		expect(mediaType('/x/a.ogg')).toBe('audio/ogg');
	});

	it('maps .opus to audio/ogg', () => {
		expect(mediaType('/x/a.opus')).toBe('audio/ogg');
	});

	it('maps .wav to audio/wav', () => {
		expect(mediaType('/x/a.wav')).toBe('audio/wav');
	});

	it('maps .aac to audio/aac', () => {
		expect(mediaType('/x/a.aac')).toBe('audio/aac');
	});
});

describe('rangeResponse', () => {
	let testFile: string;
	let testContent: Buffer;

	beforeAll(() => {
		const dir = mkdtempSync(join(tmpdir(), 'ranges-test-'));
		testFile = join(dir, 'test.mp3');
		// Create a test file with known content: each byte i contains value i%256
		testContent = Buffer.alloc(1000);
		for (let i = 0; i < 1000; i++) {
			testContent[i] = i % 256;
		}
		writeFileSync(testFile, testContent);
	});

	it('GET with no Range header returns 200 with full file and accept-ranges header', async () => {
		const res = await rangeResponse(testFile, null, false);
		expect(res.status).toBe(200);
		expect(res.headers.get('content-type')).toBe('audio/mpeg');
		expect(res.headers.get('content-length')).toBe('1000');
		expect(res.headers.get('accept-ranges')).toBe('bytes');
		const body = await res.arrayBuffer();
		expect(body.byteLength).toBe(1000);
		// Verify actual content
		const view = new Uint8Array(body);
		for (let i = 0; i < 100; i++) {
			expect(view[i]).toBe(i % 256);
		}
	});

	it('HEAD with no Range header returns 200 with same headers but no body', async () => {
		const res = await rangeResponse(testFile, null, true);
		expect(res.status).toBe(200);
		expect(res.headers.get('content-type')).toBe('audio/mpeg');
		expect(res.headers.get('content-length')).toBe('1000');
		expect(res.headers.get('accept-ranges')).toBe('bytes');
		expect(res.body).toBeNull();
	});

	it('GET with malformed Range header (spaces) falls back to 200 whole file', async () => {
		const res = await rangeResponse(testFile, 'bytes = 0-99', false);
		expect(res.status).toBe(200);
		expect(res.headers.get('content-type')).toBe('audio/mpeg');
		expect(res.headers.get('content-length')).toBe('1000');
		expect(res.headers.get('accept-ranges')).toBe('bytes');
		const body = await res.arrayBuffer();
		expect(body.byteLength).toBe(1000);
	});

	it('HEAD with malformed Range header falls back to 200 with no body', async () => {
		const res = await rangeResponse(testFile, 'bytes = 0-99', true);
		expect(res.status).toBe(200);
		expect(res.headers.get('accept-ranges')).toBe('bytes');
		expect(res.body).toBeNull();
	});

	it('GET with garbage Range header falls back to 200 whole file', async () => {
		const res = await rangeResponse(testFile, 'garbage', false);
		expect(res.status).toBe(200);
		const body = await res.arrayBuffer();
		expect(body.byteLength).toBe(1000);
	});

	it('GET with unsatisfiable range (start >= size) returns 416 with content-range header', async () => {
		const res = await rangeResponse(testFile, 'bytes=1000-1999', false);
		expect(res.status).toBe(416);
		expect(res.headers.get('content-range')).toBe('bytes */1000');
		expect(res.body).toBeNull();
	});

	it('HEAD with unsatisfiable range returns 416 with content-range header and no body', async () => {
		const res = await rangeResponse(testFile, 'bytes=1000-1999', true);
		expect(res.status).toBe(416);
		expect(res.headers.get('content-range')).toBe('bytes */1000');
		expect(res.body).toBeNull();
	});

	it('GET with valid range returns 206 partial with exact byte content', async () => {
		const res = await rangeResponse(testFile, 'bytes=100-199', false);
		expect(res.status).toBe(206);
		expect(res.headers.get('content-type')).toBe('audio/mpeg');
		expect(res.headers.get('content-range')).toBe('bytes 100-199/1000');
		expect(res.headers.get('content-length')).toBe('100');
		const body = await res.arrayBuffer();
		expect(body.byteLength).toBe(100);
		// Verify actual content matches the requested range
		const view = new Uint8Array(body);
		for (let i = 0; i < 100; i++) {
			expect(view[i]).toBe((100 + i) % 256);
		}
	});

	it('HEAD with valid range returns 206 with same headers but no body', async () => {
		const res = await rangeResponse(testFile, 'bytes=100-199', true);
		expect(res.status).toBe(206);
		expect(res.headers.get('content-type')).toBe('audio/mpeg');
		expect(res.headers.get('content-range')).toBe('bytes 100-199/1000');
		expect(res.headers.get('content-length')).toBe('100');
		expect(res.body).toBeNull();
	});

	it('GET with open-ended range returns 206 partial with correct bytes', async () => {
		const res = await rangeResponse(testFile, 'bytes=900-', false);
		expect(res.status).toBe(206);
		expect(res.headers.get('content-range')).toBe('bytes 900-999/1000');
		expect(res.headers.get('content-length')).toBe('100');
		const body = await res.arrayBuffer();
		expect(body.byteLength).toBe(100);
		const view = new Uint8Array(body);
		for (let i = 0; i < 100; i++) {
			expect(view[i]).toBe((900 + i) % 256);
		}
	});

	it('GET with suffix range returns 206 partial with correct last N bytes', async () => {
		const res = await rangeResponse(testFile, 'bytes=-50', false);
		expect(res.status).toBe(206);
		expect(res.headers.get('content-range')).toBe('bytes 950-999/1000');
		expect(res.headers.get('content-length')).toBe('50');
		const body = await res.arrayBuffer();
		expect(body.byteLength).toBe(50);
		const view = new Uint8Array(body);
		for (let i = 0; i < 50; i++) {
			expect(view[i]).toBe((950 + i) % 256);
		}
	});
});
