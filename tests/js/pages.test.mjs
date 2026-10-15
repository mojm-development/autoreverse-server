// Page behaviours of static/js/pages.js. Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { greetingFor, laterDuplicates } from '../../static/js/pages.js';

test('the greeting follows the hour of the visitor', () => {
	assert.equal(greetingFor(0), 'Guten Morgen');
	assert.equal(greetingFor(11), 'Guten Morgen');
	assert.equal(greetingFor(12), 'Guten Tag');
	assert.equal(greetingFor(17), 'Guten Tag');
	assert.equal(greetingFor(18), 'Guten Abend');
	assert.equal(greetingFor(23), 'Guten Abend');
});

test('a tile repeated by a shifted page is found, the first copy stays', () => {
	assert.deepEqual(laterDuplicates(['1', '2', '3', '3', '4', '1']), [3, 5]);
	assert.deepEqual(laterDuplicates(['1', '2']), []);
	assert.deepEqual(laterDuplicates([]), []);
});
