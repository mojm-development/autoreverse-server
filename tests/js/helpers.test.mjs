// Shortcuts, scrubber keys, formats, navigation and the visualizer's band split.
// Run: node --test tests/js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { shortcutFor, bindPlayerShortcuts } from '../../static/js/player/shortcuts.js';
import { keyTarget } from '../../static/js/player/scrubber.js';
import { formatTime, formatHMS, formatSpeed, formatHours } from '../../static/js/player/format.js';
import { isAppPage } from '../../static/js/player/navigation.js';
import { bandLayout } from '../../static/js/player/visualizer.js';

test('the space bar and k toggle', () => {
	assert.equal(shortcutFor({ key: ' ' }), 'toggle');
	assert.equal(shortcutFor({ key: 'k' }), 'toggle');
});

test('arrows seek, shift+arrows and p/n jump', () => {
	assert.equal(shortcutFor({ key: 'ArrowLeft' }), 'back');
	assert.equal(shortcutFor({ key: 'ArrowRight' }), 'forward');
	assert.equal(shortcutFor({ key: 'ArrowLeft', shiftKey: true }), 'previous');
	assert.equal(shortcutFor({ key: 'ArrowRight', shiftKey: true }), 'next');
	assert.equal(shortcutFor({ key: 'p' }), 'previous');
	assert.equal(shortcutFor({ key: 'N' }), 'next');
	assert.equal(shortcutFor({ key: 'Escape' }), 'close');
});

test('controls with focus keep their keys, and so does a slider for its arrows', () => {
	assert.equal(shortcutFor({ key: ' ', target: { tagName: 'BUTTON' } }), null);
	assert.equal(shortcutFor({ key: 'ArrowLeft', target: { tagName: 'SELECT' } }), null);
	assert.equal(shortcutFor({ key: 'k', target: { tagName: 'INPUT' } }), null);
	assert.equal(shortcutFor({ key: 'n', target: { tagName: 'DIV', isContentEditable: true } }), null);
	const slider = { tagName: 'DIV', getAttribute: (n) => (n === 'role' ? 'slider' : null) };
	assert.equal(shortcutFor({ key: 'ArrowLeft', target: slider }), null);
	assert.equal(shortcutFor({ key: ' ', target: slider }), 'toggle');
});

test('browser and system shortcuts are left alone', () => {
	assert.equal(shortcutFor({ key: ' ', metaKey: true }), null);
	assert.equal(shortcutFor({ key: 'ArrowRight', ctrlKey: true }), null);
	assert.equal(shortcutFor({ key: 'x' }), null);
});

test('binding swallows only the keys it acted on and unbinds', () => {
	let handler = null;
	const target = {
		addEventListener: (_type, fn) => (handler = fn),
		removeEventListener: () => (handler = null)
	};
	const seen = [];
	const stop = bindPlayerShortcuts((action) => {
		seen.push(action);
		return action !== 'close';
	}, target);
	let prevented = 0;
	const press = (key) => handler({ key, target: null, preventDefault: () => (prevented += 1) });
	press(' ');
	press('x');
	press('Escape');
	assert.deepEqual(seen, ['toggle', 'close']);
	assert.equal(prevented, 1);
	stop();
	assert.equal(handler, null);
});

test('scrubber keys step 5 s, page keys 30 s, home/end jump', () => {
	assert.equal(keyTarget('ArrowRight', 10, 100), 15);
	assert.equal(keyTarget('ArrowDown', 2, 100), 0);
	assert.equal(keyTarget('PageUp', 90, 100), 100);
	assert.equal(keyTarget('Home', 50, 100), 0);
	assert.equal(keyTarget('End', 50, 100), 100);
	assert.equal(keyTarget('a', 50, 100), null);
	assert.equal(keyTarget('ArrowRight', 0, 0), null);
});

test('time and speed formats', () => {
	assert.equal(formatTime(0), '0:00');
	assert.equal(formatTime(65), '1:05');
	assert.equal(formatTime(3723), '1:02:03');
	assert.equal(formatHMS(65), '0:01:05');
	assert.equal(formatSpeed(1), '1,00×');
	assert.equal(formatSpeed(1.25), '1,25×');
	assert.equal(formatHours(5400), '1.5 Std');
	assert.equal(formatHours(1500), '25 min');
});

test('only pages of the player layout are swapped in place', () => {
	assert.equal(isAppPage('<body><div\n\t\tid="page"\n\t\thx-history-elt>'), true);
	assert.equal(isAppPage('<body><main class="login">'), false);
	assert.equal(isAppPage(null), false);
});

test('visualizer bands cover the spectrum in order, one bin at least each', () => {
	const { starts, ends, tilt } = bandLayout(40, 1024, 44100);
	for (let i = 0; i < 40; i++) {
		assert.ok(ends[i] > starts[i]);
		if (i > 0) assert.ok(starts[i] >= ends[i - 1]);
	}
	assert.ok(ends[39] <= 1024);
	assert.ok(tilt[39] > tilt[0]);
});
