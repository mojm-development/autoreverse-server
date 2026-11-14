// The playlist queue (static/js/player/queue.js) and the Media Session mapping
// (static/js/player/mediasession.js). Run: node --test tests/js/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	RADIO_QUEUE,
	autoplayEnabled,
	createQueue,
	isEntryPlaying,
	playingEntryId,
	queueStep,
	radioEntries,
	radioQuery,
	setAutoplay
} from '../../static/js/player/queue.js';
import {
	bindMediaSession,
	positionState,
	sessionMetadata
} from '../../static/js/player/mediasession.js';

const album = (trackIndex = 0, position = 0) => ({
	itemId: 1,
	kind: 'album',
	hasCover: true,
	tracks: [
		{ id: 11, title: 'Eins', duration: 100 },
		{ id: 12, title: 'Zwei', duration: 100 }
	],
	chapters: [],
	trackIndex,
	position,
	speed: 1,
	playing: true
});

const book = (position = 0) => ({
	itemId: 2,
	kind: 'book',
	hasCover: false,
	tracks: [{ id: 21, title: null, duration: 600 }],
	chapters: [
		{ title: 'Anfang', start: 0, end: 300 },
		{ title: 'Ende', start: 300, end: 600 }
	],
	trackIndex: 0,
	position,
	speed: 1.5,
	playing: false
});

const entries = [
	{ entryId: '7', itemId: 1, trackId: 12, kind: 'album' },
	{ entryId: '8', itemId: 2, trackId: null, kind: 'book' },
	{ entryId: '9', itemId: 1, trackId: null, kind: 'album' }
];

test('an entry plays when its item (and track, for a single track) does', () => {
	assert.equal(isEntryPlaying(entries[0], album(1)), true);
	assert.equal(isEntryPlaying(entries[0], album(0)), false);
	assert.equal(isEntryPlaying(entries[1], book()), true);
	assert.equal(isEntryPlaying(entries[1], null), false);
});

test('next leaves a single-track entry for the next one', () => {
	const queue = createQueue(5, entries, 0);
	assert.deepEqual(queueStep(queue, album(1, 120), 1, 20), { kind: 'entry', index: 1 });
});

test('previous restarts a single-track entry past three seconds, else goes back', () => {
	const queue = createQueue(5, entries, 0);
	assert.deepEqual(queueStep(queue, album(1, 110), -1, 10), { kind: 'restart' });
	// The first entry has nothing before it: back to its start, never into the album's track 1.
	assert.deepEqual(queueStep(queue, album(1, 101), -1, 1), { kind: 'restart' });
	const second = createQueue(5, [entries[1], entries[0]], 1);
	assert.deepEqual(queueStep(second, album(1, 101), -1, 1), { kind: 'entry', index: 0 });
});

test('inside a whole book the chapters come first, then the neighbouring entries', () => {
	const queue = createQueue(5, entries, 1);
	assert.deepEqual(queueStep(queue, book(10), 1, 10), { kind: 'store' });
	assert.deepEqual(queueStep(queue, book(400), 1, 400), { kind: 'entry', index: 2 });
	assert.deepEqual(queueStep(queue, book(400), -1, 400), { kind: 'store' });
	assert.deepEqual(queueStep(queue, book(2), -1, 2), { kind: 'entry', index: 0 });
});

test('inside a whole album the tracks come first; after the last entry there is nothing', () => {
	const queue = createQueue(5, entries, 2);
	assert.deepEqual(queueStep(queue, album(0, 5), 1, 5), { kind: 'store' });
	assert.deepEqual(queueStep(queue, album(1, 105), 1, 5), { kind: 'none' });
	assert.deepEqual(queueStep(queue, album(1, 105), -1, 5), { kind: 'store' });
	assert.deepEqual(queueStep(queue, album(0, 1), -1, 1), { kind: 'entry', index: 1 });
});

test('without a queue, or when something else plays, the store decides', () => {
	assert.deepEqual(queueStep(null, album(), 1, 0), { kind: 'store' });
	const queue = createQueue(5, entries, 1);
	assert.deepEqual(queueStep(queue, album(), 1, 0), { kind: 'store' });
});

test('only the playlist that plays marks its row', () => {
	const queue = createQueue(5, entries, 0);
	assert.equal(playingEntryId(queue, '5', album(1)), '7');
	assert.equal(playingEntryId(queue, '6', album(1)), null);
	assert.equal(playingEntryId(queue, '5', album(0)), null);
	assert.equal(playingEntryId(null, '5', album(1)), null);
});

test('the lock screen shows track, artist and album for music', () => {
	const item = { title: 'Autobahn', artist: 'Kraftwerk', author: null };
	assert.deepEqual(sessionMetadata(album(1), item), {
		title: 'Zwei',
		artist: 'Kraftwerk',
		album: 'Autobahn',
		artwork: [{ src: '/items/1/cover?size=512' }]
	});
});

test('a book shows its chapter over author and title, without a cover when it has none', () => {
	const item = { title: 'Der Fluch', artist: null, author: 'Eschbach' };
	assert.deepEqual(sessionMetadata(book(320), item), {
		title: 'Ende',
		artist: 'Eschbach',
		album: 'Der Fluch',
		artwork: []
	});
	// Before the item has loaded, and without chapters: just the title.
	const plain = { ...book(0), chapters: [] };
	assert.deepEqual(sessionMetadata(plain, { title: 'Der Fluch' }), {
		title: 'Der Fluch',
		artist: '',
		album: '',
		artwork: []
	});
});

test('position state runs per track for music and over the whole item otherwise', () => {
	assert.deepEqual(positionState(album(1, 130), 30), { duration: 100, position: 30, playbackRate: 1 });
	assert.deepEqual(positionState(book(320), 320), { duration: 600, position: 320, playbackRate: 1.5 });
	// Never past the end (the API refuses it).
	assert.equal(positionState(album(1, 250), 150).position, 100);
});

test('the media session drives the store and follows it', async () => {
	const handlers = {};
	const session = {
		metadata: null,
		playbackState: 'none',
		setActionHandler: (action, handler) => (handlers[action] = handler),
		setPositionState: (state) => (session.position = state)
	};
	globalThis.navigator ??= {};
	Object.defineProperty(globalThis.navigator, 'mediaSession', { value: session, configurable: true });
	globalThis.MediaMetadata = class {
		constructor(init) {
			Object.assign(this, init);
		}
	};
	const calls = [];
	let listener = null;
	const fake = {
		current: album(1, 130),
		subscribe: (fn) => (listener = fn),
		resume: () => calls.push('resume'),
		pause: () => calls.push('pause'),
		skipBack: (s) => calls.push(['back', s]),
		skipForward: (s) => calls.push(['forward', s]),
		seek: (s) => calls.push(['seek', s]),
		seekInTrack: (s) => calls.push(['seekInTrack', s]),
		trackOffset: () => 30
	};
	bindMediaSession(fake, {
		previous: () => calls.push('previous'),
		next: () => calls.push('next'),
		fetchItem: async () => ({ title: 'Autobahn', artist: 'Kraftwerk' })
	});
	await new Promise((resolve) => setTimeout(resolve, 0));
	assert.equal(session.playbackState, 'playing');
	assert.equal(session.metadata.title, 'Zwei');
	assert.equal(session.metadata.artist, 'Kraftwerk');
	assert.deepEqual(session.position, { duration: 100, position: 30, playbackRate: 1 });

	handlers.pause();
	handlers.nexttrack();
	handlers.previoustrack();
	handlers.seekbackward({ seekOffset: 10 });
	handlers.seekto({ seekTime: 42 });
	assert.deepEqual(calls, ['pause', 'next', 'previous', ['back', 10], ['seekInTrack', 42]]);

	fake.current = null;
	listener();
	assert.equal(session.metadata, null);
	assert.equal(session.playbackState, 'none');
	delete globalThis.MediaMetadata;
});

test('radio tracks become single-track entries of their albums', () => {
	const tracks = [
		{ id: 5, title: 'A', artist: null, duration: 200, item_id: 1, item_title: 'X', item_kind: 'album' },
		{ id: 9, title: 'B', artist: 'C', duration: 100, item_id: 2, item_title: 'Y', item_kind: 'album' }
	];
	assert.deepEqual(radioEntries(tracks), [
		{ entryId: 'radio-5', itemId: 1, trackId: 5, kind: 'album' },
		{ entryId: 'radio-9', itemId: 2, trackId: 9, kind: 'album' }
	]);
	// Next inside the radio leaves a track for the next entry, as in a playlist.
	const queue = createQueue(RADIO_QUEUE, radioEntries(tracks));
	const playing = { ...album(0), itemId: 1, tracks: [{ id: 5, duration: 200 }, { id: 6, duration: 200 }] };
	assert.deepEqual(queueStep(queue, playing, 1, 10), { kind: 'entry', index: 1 });
	// The radio marks no row on a playlist's page.
	assert.equal(playingEntryId(queue, 3, playing), null);
});

test('the radio query names one seed and what was heard, the newest 500 of it', () => {
	assert.equal(radioQuery({ trackId: 7 }), 'track_id=7');
	assert.equal(radioQuery({ itemId: 3 }, [1, 2]), 'item_id=3&exclude=1&exclude=2');
	assert.equal(radioQuery({ artist: 'AC/DC & Co' }), 'artist=AC%2FDC+%26+Co');
	const many = Array.from({ length: 600 }, (_, i) => i + 1);
	const query = new URLSearchParams(radioQuery({ trackId: 7 }, many));
	const excluded = query.getAll('exclude');
	assert.equal(excluded.length, 500);
	assert.equal(excluded[0], '101');
	assert.equal(excluded.at(-1), '600');
});

test('autoplay is on unless this browser switched it off', () => {
	const values = new Map();
	const storage = {
		getItem: (key) => values.get(key) ?? null,
		setItem: (key, value) => values.set(key, value)
	};
	assert.equal(autoplayEnabled(storage), true);
	setAutoplay(storage, false);
	assert.equal(autoplayEnabled(storage), false);
	setAutoplay(storage, true);
	assert.equal(autoplayEnabled(storage), true);
	// Private mode: no storage to read or write.
	const broken = {
		getItem: () => {
			throw new Error('denied');
		},
		setItem: () => {
			throw new Error('denied');
		}
	};
	setAutoplay(broken, false);
	assert.equal(autoplayEnabled(broken), true);
});
