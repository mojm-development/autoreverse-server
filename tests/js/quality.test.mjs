// "Streaming-Qualität" in the web player (static/js/player/quality.js) and how the store
// asks for a track. Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	pickFormat,
	pickQuality,
	qualityMode,
	setQualityMode,
	trackUrl
} from '../../static/js/player/quality.js';

function memory() {
	const values = new Map();
	return {
		getItem: (key) => (values.has(key) ? values.get(key) : null),
		setItem: (key, value) => void values.set(key, String(value))
	};
}

test('the choice is kept in the browser and automatic until made', () => {
	const storage = memory();
	assert.equal(qualityMode(storage), 'auto');
	setQualityMode(storage, 'saver');
	assert.equal(qualityMode(storage), 'saver');
	setQualityMode(storage, 'original');
	assert.equal(qualityMode(storage), 'original');
	// A word nobody knows is automatic, stored or asked for.
	setQualityMode(storage, 'lossless');
	assert.equal(qualityMode(storage), 'auto');
	storage.setItem('autoreverse-quality', 'best');
	assert.equal(qualityMode(storage), 'auto');
});

test('without storage it is automatic and setting it does not throw', () => {
	const broken = {
		getItem: () => {
			throw new Error('denied');
		},
		setItem: () => {
			throw new Error('denied');
		}
	};
	assert.equal(qualityMode(broken), 'auto');
	setQualityMode(broken, 'saver');
});

test("the listener's word stands whatever the line does", () => {
	assert.equal(pickQuality('original', { saveData: true, downlink: 0.1 }), null);
	assert.equal(pickQuality('saver', { downlink: 100 }), 'low');
	assert.equal(pickQuality('saver', undefined), 'low');
});

test('automatic goes by what the browser says about the line', () => {
	assert.equal(pickQuality('auto', { downlink: 10 }), null);
	assert.equal(pickQuality('auto', { downlink: 1.5 }), null);
	assert.equal(pickQuality('auto', { downlink: 1.45 }), 'medium');
	assert.equal(pickQuality('auto', { downlink: 0.3 }), 'medium');
	assert.equal(pickQuality('auto', { downlink: 0.25 }), 'low');
	assert.equal(pickQuality('auto', { saveData: true, downlink: 10 }), 'low');
});

test('a browser that says nothing gets the file as it is', () => {
	assert.equal(pickQuality('auto', undefined), null);
	assert.equal(pickQuality('auto', null), null);
	assert.equal(pickQuality('auto', {}), null);
	assert.equal(pickQuality('auto', { downlink: 0 }), null);
	assert.equal(pickQuality('auto', { downlink: Number.NaN }), null);
});

test('opus where the browser plays it, aac elsewhere', () => {
	assert.equal(pickFormat((type) => type.includes('opus')), 'opus');
	assert.equal(pickFormat(() => false), 'aac');
});

test('the address names the quality, or nothing at all', () => {
	assert.equal(trackUrl(7, null, 'opus'), '/tracks/7/stream');
	assert.equal(trackUrl(7, 'low', 'opus'), '/tracks/7/stream?quality=low&format=opus');
	assert.equal(trackUrl(7, 'medium', 'aac'), '/tracks/7/stream?quality=medium&format=aac');
});
