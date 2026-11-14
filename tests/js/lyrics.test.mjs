// The lyrics tab's logic (static/js/player/lyrics.js). Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activeLineIndex, activeRange, createLyrics } from '../../static/js/player/lyrics.js';

const LINES = [
	{ t: 5, text: 'Erste' },
	{ t: 12.5, text: 'Zweite' },
	{ t: 12.5, text: '(second)' },
	{ t: 20, text: '' },
	{ t: 31, text: 'Dritte' }
];

test('the active line is the last one whose time has come', () => {
	assert.equal(activeLineIndex(LINES, 0), -1);
	assert.equal(activeLineIndex(LINES, 4.99), -1);
	assert.equal(activeLineIndex(LINES, 5), 0);
	assert.equal(activeLineIndex(LINES, 12.4), 0);
	assert.equal(activeLineIndex(LINES, 12.5), 2);
	assert.equal(activeLineIndex(LINES, 25), 3);
	assert.equal(activeLineIndex(LINES, 9999), 4);
	assert.equal(activeLineIndex([], 3), -1);
});

test('lines of the same moment are active together', () => {
	assert.deepEqual(activeRange(LINES, 2), [1, 2]);
	assert.deepEqual(activeRange(LINES, 0), [0, 0]);
	assert.deepEqual(activeRange(LINES, -1), [0, -1]);
});

test('a text is fetched once, and a 404 is remembered as none', async () => {
	const calls = [];
	const answers = {
		'/tracks/1/lyrics': { ok: true, status: 200, json: async () => ({ synced: true, lines: LINES, source: 'file' }) },
		'/tracks/2/lyrics': { ok: false, status: 404, json: async () => ({ detail: 'Kein Songtext vorhanden' }) },
		'/tracks/3/lyrics': { ok: false, status: 503, json: async () => ({}) }
	};
	const lyrics = createLyrics({
		fetch: async (url) => {
			calls.push(url);
			return answers[url];
		}
	});
	assert.equal((await lyrics.load(1)).lines.length, 5);
	await lyrics.load(1);
	assert.equal(await lyrics.load(2), null);
	await lyrics.load(2);
	// A server that is busy is asked again.
	assert.equal(await lyrics.load(3), null);
	await lyrics.load(3);
	assert.deepEqual(calls, ['/tracks/1/lyrics', '/tracks/2/lyrics', '/tracks/3/lyrics', '/tracks/3/lyrics']);
});
