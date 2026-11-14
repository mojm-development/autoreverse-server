// The web player's store (legacy src/lib/player.svelte.test.ts). Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	createPlayerStore,
	locate,
	trackStart,
	chapterIndexAt,
	shuffleInPlace
} from '../../static/js/player/store.js';

function session({ startPosition = 0, kind = 'album', tracks, chapters = [], hasCover } = {}) {
	return {
		session_id: 's1',
		kind,
		has_cover: hasCover,
		start_position: startPosition,
		tracks: tracks ?? [
			{ id: 11, position: 1, title: 'T1', duration: 100 },
			{ id: 22, position: 2, title: 'T2', duration: 100 },
			{ id: 33, position: 3, title: 'T3', duration: 100 }
		],
		chapters
	};
}

/** A fetch that answers /play with `body` (a fresh copy each time) and records every call. */
function fakeFetch(body) {
	const calls = [];
	const fn = async (url, init) => {
		calls.push([url, init]);
		if (String(url).startsWith('/play/')) {
			return { ok: true, json: async () => structuredClone(body) };
		}
		return { ok: true, json: async () => ({}) };
	};
	fn.calls = calls;
	return fn;
}

function fakeAudio(play = () => Promise.resolve()) {
	const el = {
		src: '',
		currentTime: 0,
		readyState: 0,
		playbackRate: 1,
		paused: 0,
		played: 0,
		play: () => {
			el.played += 1;
			return play();
		},
		pause: () => {
			el.paused += 1;
		},
		getAttribute: (name) => (name === 'src' ? el.src || null : null),
		removeAttribute: (name) => {
			if (name === 'src') el.src = '';
			el.removed = name;
		}
	};
	return el;
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function store(body = session(), preferences) {
	const fetch = fakeFetch(body);
	return { fetch, player: createPlayerStore({ fetch, preferences }) };
}

test('locate and trackStart map positions to tracks', () => {
	const tracks = [{ duration: 100 }, { duration: 50 }];
	assert.deepEqual(locate(tracks, 0), { trackIndex: 0, offset: 0 });
	assert.deepEqual(locate(tracks, 120), { trackIndex: 1, offset: 20 });
	// Past the end stays in the last track.
	assert.deepEqual(locate(tracks, 500), { trackIndex: 1, offset: 400 });
	assert.equal(trackStart(tracks, 1), 100);
	assert.equal(chapterIndexAt([{ start: 0, end: 10 }, { start: 10, end: 20 }], 15), 1);
	assert.equal(chapterIndexAt([{ start: 0, end: 10 }], 15), -1);
	const list = [1, 2, 3, 4];
	shuffleInPlace(list, () => 0);
	assert.deepEqual(list.slice().sort(), [1, 2, 3, 4]);
});

test('play() fetches the session and populates current', async () => {
	const { fetch, player } = store();
	assert.equal(await player.play(42), true);
	assert.equal(fetch.calls[0][0], '/play/42');
	assert.equal(fetch.calls[0][1].method, 'POST');
	assert.equal(player.current.sessionId, 's1');
	assert.equal(player.current.tracks.length, 3);
	assert.equal(player.current.playing, true);
});

test('play() records kind and cover, hasCover false when omitted', async () => {
	const { player } = store(session({ kind: 'book', hasCover: true }));
	await player.play(1);
	assert.equal(player.current.kind, 'book');
	assert.equal(player.current.hasCover, true);
	const other = store(session());
	await other.player.play(1);
	assert.equal(other.player.current.hasCover, false);
});

test('play() gives up on a refused session without touching the player', async () => {
	const player = createPlayerStore({
		fetch: async () => ({ ok: false, json: async () => ({ detail: 'x' }) })
	});
	assert.equal(await player.play(1), false);
	assert.equal(player.current, null);
});

test('play() resuming past the first track lands on the right track', async () => {
	const { player } = store(session({ startPosition: 150 }));
	await player.play(42);
	assert.equal(player.current.trackIndex, 1);
	assert.equal(player.current.position, 150);
});

test('pause()/resume() toggle playing without a network call', async () => {
	const { fetch, player } = store();
	await player.play(42);
	player.pause();
	assert.equal(player.current.playing, false);
	player.resume();
	assert.equal(player.current.playing, true);
	assert.equal(fetch.calls.length, 1);
});

test('subscribers hear about changes', async () => {
	const { player } = store();
	let calls = 0;
	const stop = player.subscribe(() => (calls += 1));
	await player.play(42);
	player.pause();
	assert.ok(calls >= 2);
	stop();
	const before = calls;
	player.resume();
	assert.equal(calls, before);
});

test('attachAudioElement() stops the previous element before adopting a new one', async () => {
	const { player } = store();
	await player.play(42);
	const old = fakeAudio();
	const next = fakeAudio();
	player.attachAudioElement(old);
	assert.equal(old.src, '/tracks/11/stream');
	assert.equal(old.paused, 0);
	player.attachAudioElement(next);
	assert.equal(old.paused, 1);
	assert.equal(old.removed, 'src');
	assert.equal(next.src, '/tracks/11/stream');
});

test('playTrackAt() starts a not-yet-loaded item at the requested track', async () => {
	const { player } = store(session({ startPosition: 250 }));
	await player.playTrackAt(42, 1);
	assert.equal(player.current.trackIndex, 1);
	assert.equal(player.current.position, 100);
	assert.equal(player.current.playing, true);
});

test('playTrackAt() jumps inside a loaded item without a second session, clamping', async () => {
	const { fetch, player } = store();
	await player.play(42);
	await player.playTrackAt(42, 2);
	assert.equal(player.current.trackIndex, 2);
	assert.equal(player.current.position, 200);
	await player.playTrackAt(42, 99);
	assert.equal(player.current.trackIndex, 2);
	assert.equal(fetch.calls.filter(([url]) => url.startsWith('/play/')).length, 1);
});

test('playTrackById() resolves by id and ignores unknown ids', async () => {
	const { player } = store();
	await player.playTrackById(42, 33);
	assert.equal(player.current.trackIndex, 2);
	assert.equal(player.current.position, 200);
	await player.playTrackById(42, 999);
	assert.equal(player.current.trackIndex, 2);
});

test('playFrom() seeks to an absolute position across a track boundary', async () => {
	const { player } = store();
	await player.playFrom(42, 250);
	assert.equal(player.current.trackIndex, 2);
	assert.equal(player.current.position, 250);
	assert.equal(player.current.playing, true);
});

test('next/previous track: stop at the end, restart when more than 3 s in', async () => {
	const { player } = store();
	await player.play(42);
	player.nextTrack();
	player.nextTrack();
	player.nextTrack();
	assert.equal(player.current.trackIndex, 2);
	player.seek(250);
	player.previousTrack();
	assert.equal(player.current.trackIndex, 2);
	assert.equal(player.current.position, 200);
	player.previousTrack();
	assert.equal(player.current.trackIndex, 1);
	player.seek(0);
	player.previousTrack();
	assert.equal(player.current.trackIndex, 0);
});

test('trackOffset() and seekInTrack() are relative to the current track', async () => {
	const { player } = store();
	await player.play(42);
	player.seek(130);
	assert.equal(player.trackOffset(), 30);
	player.seekInTrack(10);
	assert.equal(player.current.position, 110);
});

test('a shuffled play starts at the top of a reordered list', async () => {
	const { player } = store();
	await player.play(42, undefined, { shuffle: true, random: () => 0 });
	assert.deepEqual(
		player.current.tracks.map((t) => t.id),
		[22, 33, 11]
	);
	assert.equal(player.current.trackIndex, 0);
	assert.equal(player.current.position, 0);
});

test('reloadCurrentTrack() re-reads the track at the same index', async () => {
	const { player } = store();
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	const tracks = player.current.tracks;
	[tracks[0], tracks[1]] = [tracks[1], tracks[0]];
	player.reloadCurrentTrack();
	assert.equal(el.src, '/tracks/22/stream');
});

test('preferences come from the caller and reach a running player', async () => {
	const { player } = store(session({ kind: 'book' }), { playbackSpeed: 1.5, skipBack: 20 });
	assert.equal(player.preferences.playbackSpeed, 1.5);
	assert.equal(player.preferences.skipForward, 15);
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	assert.equal(player.current.speed, 1.5);
	player.applyPreferences({ playbackSpeed: 2, skipBack: 10, skipForward: 5 });
	assert.equal(player.current.speed, 2);
	assert.equal(el.playbackRate, 2);
	player.setSpeed(9);
	assert.equal(player.current.speed, 4);
});

test('a refused autoplay leaves the player on pause', async () => {
	const { player } = store();
	await player.play(42);
	const refusal = Object.assign(new Error('blocked'), { name: 'NotAllowedError' });
	const el = fakeAudio(() => Promise.reject(refusal));
	player.attachAudioElement(el);
	await flush();
	assert.equal(player.current.playing, false);
	// Marked, so the mini bar can offer the click the browser wants ("Hier weiterhören") …
	assert.equal(player.current.refused, true);
	// … and unmarked once it plays.
	el.onplay();
	assert.equal(player.current.refused, false);
	assert.equal(player.current.playing, true);
});

test('pauses from the element are believed, the one from a source swap is not', async () => {
	const { player } = store();
	await player.play(42);
	const el = fakeAudio(() => Promise.resolve());
	player.attachAudioElement(el);
	await flush();
	el.onpause();
	assert.equal(player.current.playing, false);
	el.onplay();
	assert.equal(player.current.playing, true);

	const pending = store();
	await pending.player.play(42);
	const swapping = fakeAudio(() => new Promise(() => {}));
	pending.player.attachAudioElement(swapping);
	swapping.onpause();
	assert.equal(pending.player.current.playing, true);
});

test('a track running out plays on; the last one stops and reports finished', async () => {
	const { fetch, player } = store(
		session({
			tracks: [
				{ id: 1, position: 1, title: 'T1', duration: 100 },
				{ id: 2, position: 2, title: 'T2', duration: 100 }
			]
		})
	);
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	await flush();
	el.onpause();
	el.onended();
	assert.equal(player.current.trackIndex, 1);
	assert.equal(el.src, '/tracks/2/stream');
	assert.equal(player.current.position, 100);
	await flush();
	assert.equal(player.current.playing, true);

	el.onpause();
	el.onended();
	assert.equal(player.current.trackIndex, 1);
	assert.equal(player.current.playing, false);
	await flush();
	const progress = fetch.calls.find(([url]) => url === '/progress/42');
	assert.match(progress[1].body, /"finished":true/);
});

test('a position set before the metadata arrives is written again once it does', async () => {
	const { player } = store(session({ startPosition: 150 }));
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	assert.equal(el.currentTime, 0);
	el.readyState = 1;
	el.onloadedmetadata();
	assert.equal(el.currentTime, 50);
});

test('jumpPrevious()/jumpNext() step by chapter for a book', async () => {
	const { player } = store(
		session({
			kind: 'book',
			startPosition: 130,
			tracks: [{ id: 1, position: 1, title: 'T1', duration: 600 }],
			chapters: [
				{ title: 'K1', start: 0, end: 100 },
				{ title: 'K2', start: 100, end: 200 },
				{ title: 'K3', start: 200, end: 600 }
			]
		})
	);
	await player.play(42);
	player.jumpPrevious();
	assert.equal(player.current.position, 100);
	player.jumpPrevious();
	assert.equal(player.current.position, 0);
	player.jumpNext();
	assert.equal(player.current.position, 100);
	player.jumpNext();
	player.jumpNext();
	assert.equal(player.current.position, 200);
});

test('nudge() steps 10 s inside the track for albums, by the preferences otherwise', async () => {
	const album = store();
	await album.player.play(42);
	album.player.seek(195);
	album.player.nudge(1);
	assert.equal(album.player.current.position, 205);
	album.player.nudge(-1);
	album.player.nudge(-1);
	// Not below the start of the track.
	assert.equal(album.player.current.position, 200);
	assert.equal(album.player.nudgeStep(-1), 10);

	const book = store(session({ kind: 'book' }), { skipBack: 30, skipForward: 15 });
	await book.player.play(1);
	book.player.seek(100);
	book.player.nudge(1);
	assert.equal(book.player.current.position, 115);
	book.player.nudge(-1);
	assert.equal(book.player.current.position, 85);
	assert.equal(book.player.nudgeStep(-1, 'album'), 10);
	assert.equal(book.player.nudgeStep(1, 'book'), 15);
});

test('switching items closes the previous session, close() ends the current one', async () => {
	// Every /play opens a new session: s1, s2, …
	let opened = 0;
	const calls = [];
	const fetch = async (url, init) => {
		calls.push([url, init]);
		if (url.startsWith('/play/')) {
			opened += 1;
			return { ok: true, json: async () => ({ ...session(), session_id: `s${opened}` }) };
		}
		return { ok: true, json: async () => ({}) };
	};
	const player = createPlayerStore({ fetch });
	await player.play(1);
	player.seek(42);
	await player.play(2);
	const close = calls.find(([url]) => url === '/sessions/s1/close');
	assert.ok(close, 'the first session is closed');
	// A jump is no listening: the position moved 42 s, nothing was audible.
	assert.deepEqual(JSON.parse(close[1].body), { position: 42, duration: 42, listened: 0 });

	await player.close();
	assert.ok(calls.some(([url]) => url === '/sessions/s2/close'));
	assert.equal(player.current, null);
});

test('getAnalyser() gives up quietly without an element or AudioContext', () => {
	const player = createPlayerStore({ fetch: fakeFetch(session()), createAudioContext: () => null });
	assert.equal(player.getAnalyser(), null);
	player.attachAudioElement(fakeAudio());
	assert.equal(player.getAnalyser(), null);
});

test('onEnded can take over when a track runs out (playlist queue)', async () => {
	const fetch = fakeFetch(
		session({
			tracks: [
				{ id: 1, position: 1, title: 'T1', duration: 100 },
				{ id: 2, position: 2, title: 'T2', duration: 100 }
			]
		})
	);
	const ended = [];
	let takeOver = true;
	const player = createPlayerStore({
		fetch,
		onEnded: (info) => {
			ended.push(info);
			return takeOver;
		}
	});
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	await flush();
	el.onended();
	assert.deepEqual(ended, [{ itemId: 42, trackId: 1, last: false }]);
	// Taken over: the store did not move on to track 2.
	assert.equal(player.current.trackIndex, 0);

	takeOver = false;
	el.onended();
	assert.equal(player.current.trackIndex, 1);
	el.onended();
	assert.deepEqual(ended.at(-1), { itemId: 42, trackId: 2, last: true });
	assert.equal(player.current.playing, false);
});

test('music plays at normal speed whatever the audiobook speed is', async () => {
	const { player } = store(session(), { playbackSpeed: 1.5 });
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	assert.equal(player.current.speed, 1);
	assert.equal(el.playbackRate, 1);
	player.applyPreferences({ playbackSpeed: 2 });
	assert.equal(el.playbackRate, 1);
	assert.equal(player.preferences.playbackSpeed, 2);
});

test('a shuffled play saves the position along the album order', async () => {
	const { fetch, player } = store();
	await player.play(42, undefined, { shuffle: true, random: () => 0 });
	const el = fakeAudio();
	player.attachAudioElement(el);
	// Shuffled order is 22, 33, 11: the last one is the album's first track.
	player.playTrackById(42, 11);
	await flush();
	el.currentTime = 5;
	el.ontimeupdate();
	assert.equal(player.current.position, 205);
	el.onpause();
	el.onended();
	await flush();
	const progress = fetch.calls.find(([url]) => url === '/progress/42');
	assert.match(progress[1].body, /"position":5,/);
});

test('closing a shuffled session reports the position in the item\'s own order', async () => {
	const calls = [];
	const tracks = [
		{ id: 1, duration: 100 },
		{ id: 2, duration: 100 },
		{ id: 3, duration: 100 }
	];
	const fetch = async (url, init) => {
		calls.push([url, init]);
		if (url.startsWith('/play/')) {
			return { ok: true, json: async () => ({ session_id: 's1', kind: 'album', start_position: 0, tracks, chapters: [] }) };
		}
		return { ok: true, json: async () => ({}) };
	};
	const player = createPlayerStore({ fetch });
	// `random: () => 0` moves the first track to the end, so the one played first is not track 1.
	await player.play(7, undefined, { shuffle: true, random: () => 0 });
	const first = player.current.tracks[0];
	player.seek(5);
	const expected = first.albumStart + 5;
	await player.close();
	const close = calls.find(([url]) => url === '/sessions/s1/close');
	assert.equal(JSON.parse(close[1].body).position, expected);
	assert.notEqual(expected, 5);
});

/** Lets the fake element play on to `seconds`, one `timeupdate` at a time. */
function playTo(el, seconds, step = 0.25) {
	while (el.currentTime < seconds) {
		el.currentTime = Math.min(seconds, el.currentTime + step);
		el.ontimeupdate();
	}
}

test('listened counts what ran through the element, not where the position went', async () => {
	const { fetch, player } = store(session({ startPosition: 20 }));
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	// Resumed at 20 s: the element is put there, which is no listening.
	el.currentTime = 20;
	el.ontimeupdate();
	assert.equal(player.current.listened, 0);
	playTo(el, 30);
	assert.equal(player.current.listened, 10);

	// A jump ahead by a track and a half, then five more seconds.
	player.seek(250);
	el.currentTime = 50;
	el.onseeking();
	el.ontimeupdate();
	playTo(el, 55);
	assert.equal(player.current.listened, 15);

	// A jump back, which the distance from the start would count as nothing at all.
	player.seek(0);
	el.currentTime = 0;
	el.ontimeupdate();
	playTo(el, 8);
	assert.equal(player.current.listened, 23);
	assert.equal(player.current.position, 8);

	await player.close();
	const close = fetch.calls.find(([url]) => url === '/sessions/s1/close');
	// The same seconds by track: the first and the last stretch were in track 11, the jump
	// landed in track 33.
	assert.deepEqual(JSON.parse(close[1].body), {
		position: 8,
		duration: 8,
		listened: 23,
		tracks: [
			{ track_id: 11, seconds: 18 },
			{ track_id: 33, seconds: 5 }
		]
	});
});

test('listened leaves out a jump nobody announced and anything while paused', async () => {
	const { player } = store();
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	// Let play() settle, or the store takes the pause below for its own track switch.
	await flush();
	el.ontimeupdate();
	playTo(el, 4);
	// The element lands somewhere else without a `seeking` event.
	el.currentTime = 90;
	el.ontimeupdate();
	assert.equal(player.current.listened, 4);
	playTo(el, 92);
	assert.equal(player.current.listened, 6);
	// Paused and scrubbed in small steps: the element moves, nothing is audible.
	el.onpause();
	el.currentTime = 93;
	el.ontimeupdate();
	assert.equal(player.current.listened, 6);
	// At four times the speed the steps are longer and still count.
	el.onplay();
	playTo(el, 99, 1);
	assert.equal(player.current.listened, 12);
	assert.deepEqual(player.closeBody(), {
		position: 99,
		duration: 99,
		listened: 12,
		tracks: [{ track_id: 11, seconds: 12 }]
	});
});

test('listened runs on across tracks and starts over with a new session', async () => {
	const { player } = store();
	await player.play(42);
	const el = fakeAudio();
	player.attachAudioElement(el);
	el.ontimeupdate();
	playTo(el, 100);
	el.onended();
	// The next track starts at zero: a new source, not a hundred seconds backwards.
	el.currentTime = 0;
	el.ontimeupdate();
	playTo(el, 3);
	assert.equal(player.current.trackIndex, 1);
	assert.equal(player.current.listened, 103);
	// Each track gets what was heard of it; together they are `listened`.
	assert.deepEqual(player.closeBody().tracks, [
		{ track_id: 11, seconds: 100 },
		{ track_id: 22, seconds: 3 }
	]);
	await player.play(43);
	assert.equal(player.current.listened, 0);
	assert.equal('tracks' in player.closeBody(), false);
	assert.equal(createPlayerStore().closeBody(), null);
});

test('a single track counts as listening but never as progress in its album', async () => {
	const { fetch, player } = store();
	const el = fakeAudio();
	player.attachAudioElement(el);
	await player.playTrackById(42, 22, { single: true });
	assert.equal(player.current.single, true);
	assert.equal(player.current.trackIndex, 1);
	playTo(el, 10);

	await player.flushProgress(false);
	el.onended();
	await flush();
	assert.equal(fetch.calls.filter(([url]) => String(url).startsWith('/progress/')).length, 0);
	// Closing sends no position, so the server leaves the album's progress alone. (The first
	// `timeupdate` after a jump only sets the mark, hence a step short of ten seconds.)
	const body = { position: null, listened: 9.75, tracks: [{ track_id: 22, seconds: 9.75 }] };
	assert.deepEqual(player.closeBody(), body);
	await player.close();
	const closed = fetch.calls.find(([url]) => String(url).endsWith('/close'));
	assert.deepEqual(JSON.parse(closed[1].body), body);
});

test('going from one track of an album to the album itself opens a new session', async () => {
	const { fetch, player } = store();
	player.attachAudioElement(fakeAudio());
	const plays = () => fetch.calls.filter(([url]) => String(url).startsWith('/play/')).length;
	await player.playTrackById(42, 22, { single: true });
	await player.playTrackById(42, 33, { single: true });
	assert.equal(plays(), 1);
	// The album page's own track row: the album now plays as an album.
	await player.playTrackById(42, 11);
	assert.equal(plays(), 2);
	assert.equal(player.current.single, false);
	assert.equal(player.current.trackIndex, 0);
	await player.playTrackById(42, 22, { single: true });
	assert.equal(plays(), 3);
	assert.equal(player.current.single, true);
});

// --- Radio stations: live, without session, progress or anything after them ------------------

const STATION = { id: 7, name: 'Jazz Eins', hasLogo: true };

test('a station plays live from the server without a session or progress', async () => {
	const { fetch, player } = store();
	const el = fakeAudio();
	player.attachAudioElement(el);
	assert.equal(player.playStation(STATION), true);
	assert.equal(el.src, '/radio/stations/7/stream');
	assert.equal(el.played, 1);
	const current = player.current;
	assert.equal(current.live, true);
	assert.equal(current.kind, 'station');
	assert.equal(current.stationId, 7);
	assert.equal(current.itemId, null);
	assert.equal(current.tracks[0].title, 'Jazz Eins');
	assert.equal(current.playing, true);

	// Time passes on the element; nothing of it is saved or booked.
	el.currentTime = 0;
	el.ontimeupdate();
	el.currentTime = 30;
	el.ontimeupdate();
	await player.flushProgress(false);
	await player.close();
	await flush();
	assert.deepEqual(fetch.calls, []);
	assert.equal(player.current, null);
});

test('a station closes what played before, and an album after it closes nothing', async () => {
	const { fetch, player } = store();
	player.attachAudioElement(fakeAudio());
	await player.play(42);
	player.playStation(STATION);
	await flush();
	const closes = () => fetch.calls.filter(([url]) => String(url).endsWith('/close'));
	assert.equal(closes().length, 1);
	assert.equal(closes()[0][0], '/sessions/s1/close');

	// Back to the album: the station had no session to close.
	await player.play(42);
	await flush();
	assert.equal(closes().length, 1);
	assert.equal(player.current.live, undefined);
	assert.equal(player.current.itemId, 42);
});

test('a live stream cannot be seeked, skipped or stepped through', () => {
	const { player } = store();
	const el = fakeAudio();
	player.attachAudioElement(el);
	player.playStation(STATION);
	el.readyState = 4;
	el.currentTime = 12;
	const loads = el.played;
	player.seek(100);
	player.skipBack();
	player.skipForward();
	player.nudge(1);
	player.seekInTrack(5);
	player.jumpPrevious();
	player.jumpNext();
	player.previousTrack();
	player.nextTrack();
	assert.equal(el.currentTime, 12);
	assert.equal(el.played, loads);
	assert.equal(el.src, '/radio/stations/7/stream');
	// The audiobook speed does not apply either.
	player.setSpeed(2);
	assert.equal(player.current.speed, 1);
	assert.equal(el.playbackRate, 1);
});

test('pausing a station lets go of the stream and resuming tunes in again', async () => {
	const { player } = store();
	const el = fakeAudio();
	let reset = 0;
	el.load = () => {
		reset += 1;
	};
	player.attachAudioElement(el);
	player.playStation(STATION);
	player.setNowPlaying(7, { title: 'Autobahn', artist: 'Kraftwerk', raw: 'Kraftwerk - Autobahn' });

	player.pause();
	assert.equal(player.current.playing, false);
	assert.equal(el.src, '');
	assert.equal(reset, 1);
	// The title belonged to the stream that is gone.
	assert.equal(player.current.nowPlaying, null);

	player.resume();
	assert.equal(el.src, '/radio/stations/7/stream');
	assert.equal(player.current.playing, true);
	// Also after a pause the element made itself (a headset button): from "now", not the buffer.
	await flush();
	el.onpause();
	assert.equal(player.current.playing, false);
	const played = el.played;
	player.toggle();
	assert.equal(el.played, played + 1);
	assert.equal(player.current.playing, true);
});

test('a station that ends or fails stops, and nothing plays on after it', () => {
	const ended = [];
	const fetch = fakeFetch(session());
	const player = createPlayerStore({ fetch, onEnded: (e) => ended.push(e) });
	const el = fakeAudio();
	player.attachAudioElement(el);
	player.playStation(STATION);
	el.onended();
	assert.equal(player.current.playing, false);
	// The queue and the autoplay never hear of it.
	assert.deepEqual(ended, []);

	player.resume();
	assert.equal(player.current.playing, true);
	el.onerror();
	assert.equal(player.current.playing, false);
	assert.deepEqual(fetch.calls, []);
});

test('the running title belongs to the station that plays', () => {
	const { player } = store();
	player.attachAudioElement(fakeAudio());
	let notified = 0;
	player.playStation(STATION);
	player.subscribe(() => {
		notified += 1;
	});
	const now = { title: 'Autobahn', artist: 'Kraftwerk', raw: 'Kraftwerk - Autobahn', listening: true };
	player.setNowPlaying(8, now);
	assert.equal(player.current.nowPlaying, null);
	player.setNowPlaying(7, now);
	assert.deepEqual(player.current.nowPlaying, {
		title: 'Autobahn',
		artist: 'Kraftwerk',
		raw: 'Kraftwerk - Autobahn'
	});
	// The same answer again changes nothing and redraws nothing.
	player.setNowPlaying(7, now);
	assert.equal(notified, 1);
	// No title known (any more).
	player.setNowPlaying(7, { title: null, artist: null, raw: null, listening: true });
	assert.equal(player.current.nowPlaying, null);
});
