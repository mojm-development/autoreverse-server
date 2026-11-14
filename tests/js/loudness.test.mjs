// "Lautstärke angleichen" in the web player (static/js/player/loudness.js) and how the store
// applies it. Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	gainFactor,
	normalizeEnabled,
	setNormalize,
	volumeFor
} from '../../static/js/player/loudness.js';
import { createPlayerStore } from '../../static/js/player/store.js';

const close = (actual, expected) =>
	assert.ok(Math.abs(actual - expected) < 1e-6, `${actual} ≠ ${expected}`);

test('a negative gain turns the track down by that many dB', () => {
	close(gainFactor({ gain_db: -6, album_gain_db: -3, peak: 1 }, false), 10 ** (-6 / 20));
	close(gainFactor({ gain_db: -6, album_gain_db: -3, peak: 1 }, true), 10 ** (-3 / 20));
	close(gainFactor({ gain_db: 0, album_gain_db: 0, peak: 0.5 }, false), 1);
});

test('each value stands in for the other, and nothing known means no change', () => {
	close(gainFactor({ gain_db: -6, album_gain_db: null, peak: null }, true), 10 ** (-6 / 20));
	close(gainFactor({ gain_db: null, album_gain_db: -4, peak: null }, false), 10 ** (-4 / 20));
	assert.equal(gainFactor({ gain_db: null, album_gain_db: null, peak: null }, true), 1);
	// An older server sends no fields at all.
	assert.equal(gainFactor({ id: 1, duration: 100 }, false), 1);
	assert.equal(gainFactor(undefined, false), 1);
	assert.equal(gainFactor({ gain_db: 'laut', album_gain_db: NaN }, false), 1);
});

test('a boost stops where the peak would clip, and needs a peak at all', () => {
	// +6 dB wanted, but the peak leaves room for a factor of 1.25 only.
	close(gainFactor({ gain_db: 6, peak: 0.8 }, false), 1.25);
	// Room enough: the full gain.
	close(gainFactor({ gain_db: 6, peak: 0.25 }, false), 10 ** (6 / 20));
	// Already at or above full scale: no boost.
	assert.equal(gainFactor({ gain_db: 6, peak: 1.02 }, false), 1);
	// Without a peak nobody knows how much room there is.
	assert.equal(gainFactor({ gain_db: 6, peak: null }, false), 1);
	assert.equal(gainFactor({ gain_db: 6, peak: 0 }, false), 1);
	// The peak never limits turning down.
	close(gainFactor({ gain_db: -6, peak: 1.3 }, false), 10 ** (-6 / 20));
});

test('the element volume ends at 1: the web player only turns down', () => {
	assert.equal(volumeFor({ gain_db: 6, peak: 0.25 }, false), 1);
	close(volumeFor({ gain_db: -9.5, peak: 1 }, false), 10 ** (-9.5 / 20));
	assert.equal(volumeFor({}, false), 1);
});

test('levelling is on unless this browser switched it off', () => {
	const values = new Map();
	const storage = {
		getItem: (key) => values.get(key) ?? null,
		setItem: (key, value) => values.set(key, value)
	};
	assert.equal(normalizeEnabled(storage), true);
	setNormalize(storage, false);
	assert.equal(normalizeEnabled(storage), false);
	setNormalize(storage, true);
	assert.equal(normalizeEnabled(storage), true);
	const broken = {
		getItem: () => {
			throw new Error('denied');
		},
		setItem: () => {
			throw new Error('denied');
		}
	};
	setNormalize(broken, false);
	assert.equal(normalizeEnabled(broken), true);
});

// --- in the store ------------------------------------------------------------------------

const TRACKS = [
	{ id: 11, title: 'Laut', duration: 100, gain_db: -10, album_gain_db: -8, peak: 1 },
	{ id: 22, title: 'Leise', duration: 100, gain_db: -4, album_gain_db: -8, peak: 0.7 },
	{ id: 33, title: 'Ungemessen', duration: 100, gain_db: null, album_gain_db: null, peak: null }
];

function setup({ kind = 'album', normalize } = {}) {
	const fetch = async (url) => ({
		ok: true,
		json: async () =>
			String(url).startsWith('/play/')
				? { session_id: 's1', kind, start_position: 0, tracks: structuredClone(TRACKS), chapters: [] }
				: {}
	});
	const el = {
		src: '',
		volume: 1,
		currentTime: 0,
		readyState: 0,
		playbackRate: 1,
		play: () => Promise.resolve(),
		pause: () => {},
		load: () => {},
		getAttribute: (name) => (name === 'src' ? el.src || null : null),
		removeAttribute: (name) => {
			if (name === 'src') el.src = '';
		}
	};
	const player = createPlayerStore({ fetch, normalize });
	player.attachAudioElement(el);
	return { el, player };
}

test('an album played as such keeps one volume across its tracks', async () => {
	const { el, player } = setup();
	await player.play(42);
	close(el.volume, 10 ** (-8 / 20));
	player.nextTrack();
	assert.equal(el.src, '/tracks/22/stream');
	// The album's value again: no step between two tracks of the album.
	close(el.volume, 10 ** (-8 / 20));
	// A track nobody measured yet plays as it is.
	player.nextTrack();
	assert.equal(el.volume, 1);
});

test('a single track out of a playlist or the Titel-Radio takes its own value', async () => {
	const { el, player } = setup();
	await player.playTrackById(42, 11, { single: true });
	close(el.volume, 10 ** (-10 / 20));
	await player.playTrackById(42, 22, { single: true });
	close(el.volume, 10 ** (-4 / 20));
});

test('spoken word and radio stations are left alone', async () => {
	for (const kind of ['book', 'podcast']) {
		const { el, player } = setup({ kind });
		await player.play(42);
		assert.equal(el.volume, 1, kind);
	}
	const { el, player } = setup();
	await player.play(42);
	assert.ok(el.volume < 1);
	// A station after music must not inherit the last track's volume.
	player.playStation({ id: 7, name: 'Jazz Eins' });
	assert.equal(el.volume, 1);
});

test('the switch takes effect at once, in both directions', async () => {
	let on = false;
	const { el, player } = setup({ normalize: () => on });
	await player.play(42);
	assert.equal(el.volume, 1);
	on = true;
	player.applyVolume();
	close(el.volume, 10 ** (-8 / 20));
	on = false;
	player.applyVolume();
	assert.equal(el.volume, 1);
});

test('closing the player gives the element its full volume back', async () => {
	const { el, player } = setup();
	await player.play(42);
	assert.ok(el.volume < 1);
	await player.close();
	assert.equal(el.volume, 1);
});

test("the listener's own volume scales the levelled one and survives the track", async () => {
	const { el, player } = setup();
	let notified = 0;
	player.subscribe(() => notified++);
	assert.equal(player.volume, 1);
	player.setVolume(0.5);
	assert.equal(player.volume, 0.5);
	assert.equal(el.volume, 0.5);
	assert.equal(notified, 1);
	await player.play(42);
	close(el.volume, 0.5 * 10 ** (-8 / 20));
	player.nextTrack();
	player.nextTrack();
	assert.equal(el.volume, 0.5);
	// Out of range is brought in, nonsense ignored, the same value tells nobody.
	player.setVolume(7);
	assert.equal(player.volume, 1);
	player.setVolume('laut');
	assert.equal(player.volume, 1);
	const before = notified;
	player.setVolume(1);
	assert.equal(notified, before);
});
