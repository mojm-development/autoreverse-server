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
	const { player } = store(session(), { playbackSpeed: 1.5, skipBack: 20 });
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
	player.attachAudioElement(fakeAudio(() => Promise.reject(refusal)));
	await flush();
	assert.equal(player.current.playing, false);
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
	assert.deepEqual(JSON.parse(close[1].body), { position: 42, duration: 42 });

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
