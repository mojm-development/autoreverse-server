// Remote control, the browser's logic (static/js/player/devices.js): what the web player
// reports, where a watched device stands, how a hand-over is played, which tab holds the stream.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	MAX_QUEUE,
	createElection,
	deviceId,
	deviceName,
	differsNotably,
	localState,
	projectedPosition,
	shouldReport,
	takeOverPlan,
	watchedDevice
} from '../../static/js/player/devices.js';
import { deviceLine } from '../../static/js/player/devices-ui.js';
import { RADIO_QUEUE, createQueue } from '../../static/js/player/queue.js';

const NOW = Date.parse('2026-11-13T12:00:00.000Z');

function album(overrides = {}) {
	return {
		itemId: 7,
		kind: 'album',
		playing: true,
		single: false,
		tracks: [
			{ id: 70, title: 'Human Behaviour', artist: 'Björk', duration: 252 },
			{ id: 71, title: 'Crying', artist: null, duration: 289 },
			{ id: 72, title: 'Venus as a Boy', artist: null, duration: 282 }
		],
		chapters: [],
		trackIndex: 1,
		position: 252 + 30,
		...overrides
	};
}

function memoryStorage(initial = {}) {
	const values = new Map(Object.entries(initial));
	return {
		getItem: (key) => values.get(key) ?? null,
		setItem: (key, value) => values.set(key, value),
		values
	};
}

test('the device id is made once and kept', () => {
	const storage = memoryStorage();
	let made = 0;
	const create = () => `device-${++made}-abcdef`;
	assert.equal(deviceId(storage, create), 'device-1-abcdef');
	assert.equal(deviceId(storage, create), 'device-1-abcdef');
	assert.equal(made, 1);
	// Something the server would refuse is replaced.
	assert.equal(deviceId(memoryStorage({ 'autoreverse-device-id': 'x y' }), create), 'device-2-abcdef');
});

test('without storage the id still fits the contract', () => {
	const broken = {
		getItem: () => {
			throw new Error('denied');
		}
	};
	assert.match(deviceId(broken), /^[A-Za-z0-9-]{8,64}$/);
});

test('the device is named after browser and platform', () => {
	assert.equal(
		deviceName('Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0'),
		'Firefox auf Linux'
	);
	assert.equal(
		deviceName('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15'),
		'Safari auf macOS'
	);
	assert.equal(
		deviceName('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0'),
		'Edge auf Windows'
	);
	assert.equal(
		deviceName('Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36'),
		'Chrome auf Android'
	);
	assert.equal(deviceName(''), 'Browser');
});

test('nothing playing is an empty state', () => {
	assert.deepEqual(localState({ current: null, volume: 0.5, now: NOW }), {
		playing: false,
		media: null,
		position: 0,
		duration: 0,
		volume: 0.5,
		shuffle: false,
		repeat: 'off',
		queue: [],
		context: null,
		at: '2026-11-13T12:00:00.000Z'
	});
});

test('an album reports the track, the place in it and the tracks that follow', () => {
	const state = localState({ current: album(), item: { title: 'Debut', artist: 'Björk' }, now: NOW });
	assert.deepEqual(state.media, {
		kind: 'track',
		item_id: 7,
		track_id: 71,
		station_id: null,
		title: 'Crying',
		artist: 'Björk',
		item_title: 'Debut'
	});
	assert.equal(state.playing, true);
	assert.equal(state.position, 30);
	assert.equal(state.duration, 289);
	assert.deepEqual(state.queue, [
		{ item_id: 7, track_id: 71 },
		{ item_id: 7, track_id: 72 }
	]);
	assert.equal(state.context, 'Debut');
});

test('a track out of a queue is followed by the queue, not by its album', () => {
	const entries = [
		{ entryId: 'a', itemId: 9, trackId: 90, kind: 'album' },
		{ entryId: 'b', itemId: 7, trackId: 71, kind: 'album' },
		{ entryId: 'c', itemId: 3, trackId: null, kind: 'book' }
	];
	const queue = createQueue(RADIO_QUEUE, entries, 1);
	const state = localState({ current: album({ single: true }), queue, now: NOW });
	assert.deepEqual(state.queue, [
		{ item_id: 7, track_id: 71 },
		{ item_id: 3, track_id: null }
	]);
	assert.equal(state.context, 'Titel-Radio');
	// A queue that is not what plays any more says nothing.
	const stale = createQueue('5', entries, 0);
	assert.equal(localState({ current: album(), queue: stale, now: NOW }).queue.length, 2);
});

test('the queue is cut to what the server takes', () => {
	const tracks = Array.from({ length: 400 }, (_, i) => ({ id: i + 1, title: `T${i}`, duration: 10 }));
	const state = localState({ current: album({ tracks, trackIndex: 0, position: 0 }), now: NOW });
	assert.equal(state.queue.length, MAX_QUEUE);
	assert.deepEqual(state.queue[0], { item_id: 7, track_id: 1 });
});

test('a book reports the item and the place in the whole', () => {
	const current = {
		itemId: 4,
		kind: 'book',
		playing: false,
		tracks: [
			{ id: 1, title: 'Teil 1', duration: 1000 },
			{ id: 2, title: 'Teil 2', duration: 2000 }
		],
		chapters: [],
		trackIndex: 1,
		position: 1500
	};
	const state = localState({ current, item: { title: 'Der Zauberberg', author: 'Thomas Mann' }, now: NOW });
	assert.deepEqual(state.media, {
		kind: 'book',
		item_id: 4,
		track_id: null,
		station_id: null,
		title: 'Der Zauberberg',
		artist: 'Thomas Mann',
		item_title: null
	});
	assert.equal(state.position, 1500);
	assert.equal(state.duration, 3000);
	assert.deepEqual(state.queue, [{ item_id: 4, track_id: null }]);
	assert.equal(localState({ current: { ...current, kind: 'episode' }, now: NOW }).media.kind, 'episode');
});

test('a station reports itself without a position or a queue', () => {
	const current = {
		itemId: null,
		stationId: 3,
		kind: 'station',
		live: true,
		playing: true,
		tracks: [{ id: null, title: 'Radio Eins', duration: 0 }],
		trackIndex: 0,
		position: 0,
		nowPlaying: { raw: 'Björk - Hyperballad' }
	};
	const state = localState({ current, now: NOW });
	assert.equal(state.media.kind, 'station');
	assert.equal(state.media.station_id, 3);
	assert.equal(state.media.title, 'Radio Eins');
	assert.equal(state.media.item_title, 'Björk - Hyperballad');
	assert.deepEqual([state.position, state.duration, state.queue], [0, 0, []]);
});

test('progress alone is reported every ten seconds, a change at once', () => {
	const at = (seconds) => NOW + seconds * 1000;
	const state = (position, seconds, overrides = {}) =>
		localState({ current: album({ position: 252 + position, ...overrides }), now: at(seconds) });
	const sent = { state: state(30, 0), at: NOW };
	assert.equal(shouldReport(null, state(30, 0), NOW), true);
	assert.equal(shouldReport(sent, state(34, 4), at(4)), false);
	assert.equal(shouldReport(sent, state(40, 10), at(10)), true);
	// A seek, a pause, another track, another volume.
	assert.equal(shouldReport(sent, state(120, 4), at(4)), true);
	assert.equal(shouldReport(sent, state(34, 4, { playing: false }), at(4)), true);
	assert.equal(shouldReport(sent, state(0, 4, { trackIndex: 2, position: 541 }), at(4)), true);
	const louder = localState({ current: album(), volume: 0.4, now: at(4) });
	assert.equal(differsNotably(sent.state, louder), true);
	// Paused, nothing is reported however long it lasts.
	const paused = { state: state(30, 0, { playing: false }), at: NOW };
	assert.equal(shouldReport(paused, state(30, 600, { playing: false }), at(600)), false);
});

test('a watched position runs on while it plays', () => {
	const state = { playing: true, position: 100, duration: 252, media: { kind: 'track' }, at: '2026-11-13T12:00:00.000Z' };
	assert.equal(projectedPosition(state, NOW), 100);
	assert.equal(projectedPosition(state, NOW + 12_500), 112.5);
	// Not past the end, not before the report (this clock runs behind the device's).
	assert.equal(projectedPosition(state, NOW + 900_000), 252);
	assert.equal(projectedPosition(state, NOW - 60_000), 100);
	assert.equal(projectedPosition({ ...state, playing: false }, NOW + 12_500), 100);
	assert.equal(projectedPosition({ ...state, duration: 0 }, NOW + 12_500), 112.5);
	assert.equal(projectedPosition({ ...state, media: { kind: 'station' }, position: 0 }, NOW + 12_500), 0);
	assert.equal(projectedPosition({ ...state, at: 'kaputt' }, NOW), 100);
	assert.equal(projectedPosition(null, NOW), 0);
});

test('the mini bar watches the active other device', () => {
	const media = { kind: 'track', title: 'x' };
	const self = { id: 'self-0000', active: true, state: { playing: true, media } };
	const idle = { id: 'idle-0000', active: false, state: null };
	const paused = { id: 'paused-00', active: false, state: { playing: false, media } };
	const playing = { id: 'playing-0', active: false, state: { playing: true, media } };
	const active = { id: 'active-00', active: true, state: { playing: false, media } };
	assert.equal(watchedDevice([self, idle, paused], 'self-0000'), null);
	assert.equal(watchedDevice([self, idle, paused, playing], 'self-0000'), playing);
	assert.equal(watchedDevice([idle, playing, active], 'self-0000'), active);
	assert.equal(watchedDevice([{ ...active, state: { playing: false, media: null } }], 'self-0000'), null);
	assert.equal(watchedDevice(undefined, 'self-0000'), null);
});

test('a handed-over state becomes a plan to play', () => {
	const base = { playing: true, position: 42.5, duration: 252, queue: [] };
	assert.equal(takeOverPlan({ ...base, media: null }), null);
	assert.deepEqual(takeOverPlan({ ...base, media: { kind: 'station', station_id: 3, title: 'Radio Eins' } }), {
		kind: 'station',
		stationId: 3,
		title: 'Radio Eins'
	});
	assert.deepEqual(takeOverPlan({ ...base, media: { kind: 'book', item_id: 4, title: 'Buch' } }), {
		kind: 'item',
		itemId: 4,
		itemKind: 'book',
		position: 42.5
	});
	const music = takeOverPlan({
		...base,
		media: { kind: 'track', item_id: 7, track_id: 71, title: 'Crying' },
		queue: [{ item_id: 7, track_id: 71 }, { item_id: 9, track_id: 90 }, { item_id: null, track_id: 5 }]
	});
	assert.deepEqual(music, {
		kind: 'tracks',
		entries: [
			{ entryId: 'remote-0', itemId: 7, trackId: 71, kind: 'album' },
			{ entryId: 'remote-1', itemId: 9, trackId: 90, kind: 'album' }
		],
		trackId: 71,
		position: 42.5
	});
	// No queue reported: the medium itself. An album as such keeps the running track's id.
	assert.deepEqual(takeOverPlan({ ...base, media: { kind: 'album', item_id: 7, track_id: 71, title: 'Debut' } }).entries, [
		{ entryId: 'remote-0', itemId: 7, trackId: 71, kind: 'album' }
	]);
	assert.equal(takeOverPlan({ ...base, media: { kind: 'track', item_id: null, title: 'x' } }), null);
	assert.equal(takeOverPlan({ ...base, media: { kind: 'station', station_id: null, title: 'x' } }), null);
});

test('a row of the panel says what the device does', () => {
	assert.equal(deviceLine({ kind: 'phone', state: null }), 'Telefon · bereit');
	assert.equal(
		deviceLine({ kind: 'desktop', state: { playing: true, media: { title: 'Crying', artist: 'Björk' } } }),
		'Rechner · spielt: Crying · Björk'
	);
	assert.equal(
		deviceLine({ kind: 'car', state: { playing: false, media: { title: 'Der Zauberberg', artist: null } } }),
		'Auto · pausiert: Der Zauberberg'
	);
});

/** Tabs of one browser: their elections, wired the way a BroadcastChannel wires them. */
function browser() {
	const tabs = [];
	let clock = 1000;
	const timers = [];
	const world = {
		tabs,
		advance(ms) {
			clock += ms;
			for (const timer of timers.splice(0)) {
				if (timer.at <= clock) timer.run();
				else timers.push(timer);
			}
		},
		open(id, { playing = false, visible = true } = {}) {
			const tab = { id, playing, visible, frozen: false, changes: [] };
			tab.election = createElection({
				tabId: id,
				post: (message) => {
					for (const other of tabs) {
						if (other !== tab && !other.frozen) other.election.receive(structuredClone(message));
					}
				},
				onChange: (holding) => tab.changes.push(holding),
				now: () => clock,
				later: (run, delay) => timers.push({ at: clock + delay, run }),
				random: () => 0.5,
				isPlaying: () => tab.playing,
				isVisible: () => tab.visible
			});
			tabs.push(tab);
			tab.election.start();
			return tab;
		},
		holders: () => tabs.filter((tab) => tab.election.holding).map((tab) => tab.id)
	};
	return world;
}

test('a single tab holds the stream after nobody answered', () => {
	const world = browser();
	const tab = world.open('a');
	assert.equal(tab.election.holding, false);
	world.advance(300);
	assert.deepEqual(world.holders(), ['a']);
	assert.deepEqual(tab.changes, [true]);
});

test('a second tab leaves the stream to the first', () => {
	const world = browser();
	world.open('a');
	world.advance(300);
	const second = world.open('b');
	world.advance(300);
	assert.deepEqual(world.holders(), ['a']);
	assert.deepEqual(second.changes, []);
	// Beacons change nothing, however long.
	for (let i = 0; i < 10; i++) {
		world.advance(5000);
		for (const tab of world.tabs) tab.election.tick();
	}
	assert.deepEqual(world.holders(), ['a']);
});

test('the tab that starts to play takes the stream, once', () => {
	const world = browser();
	const first = world.open('a');
	world.advance(300);
	const second = world.open('b');
	world.advance(300);
	second.playing = true;
	second.election.wantHold();
	assert.deepEqual(world.holders(), ['b']);
	assert.deepEqual(first.changes, [true, false]);
	assert.deepEqual(second.changes, [true]);
	// No tug of war afterwards.
	for (let i = 0; i < 5; i++) {
		world.advance(5000);
		for (const tab of world.tabs) tab.election.tick();
	}
	assert.deepEqual(world.holders(), ['b']);
	assert.deepEqual(first.changes, [true, false]);
});

test('two tabs opened at the same moment settle on one', () => {
	const world = browser();
	world.open('a');
	world.open('b');
	world.advance(300);
	assert.equal(world.holders().length, 1);
});

test('a tab that wakes up believing it holds the stream yields to the one that does', () => {
	const world = browser();
	const sleeper = world.open('a');
	world.advance(300);
	sleeper.frozen = true;
	const awake = world.open('b');
	world.advance(300);
	assert.deepEqual(world.holders(), ['a', 'b']);
	sleeper.frozen = false;
	world.advance(5000);
	awake.election.tick();
	assert.deepEqual(world.holders(), ['b']);
});

test('an idle tab never takes the stream from the one that plays', () => {
	const world = browser();
	const player = world.open('a', { playing: true });
	world.advance(300);
	const idle = world.open('b');
	world.advance(300);
	// The idle tab was asleep and thinks the holder is gone.
	world.advance(60_000);
	idle.election.tick();
	assert.deepEqual(world.holders(), ['a']);
	assert.deepEqual(player.changes, [true]);
});

test('when the holder leaves, another tab takes over; a hidden one does not guess', () => {
	const world = browser();
	const first = world.open('a');
	world.advance(300);
	const second = world.open('b');
	world.advance(300);
	first.election.leave();
	world.tabs.splice(0, 1);
	world.advance(300);
	assert.deepEqual(world.holders(), ['b']);

	// A holder that vanishes without a word is replaced by a visible tab only.
	const third = world.open('c', { visible: false });
	world.advance(300);
	second.frozen = true;
	world.tabs.splice(0, 1);
	world.advance(60_000);
	third.election.tick();
	assert.equal(third.election.holding, false);
	third.visible = true;
	third.election.tick();
	assert.equal(third.election.holding, true);
});
