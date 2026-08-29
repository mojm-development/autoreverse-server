import { describe, it, expect, vi, afterEach } from 'vitest';
import { createPlayerStore } from './player.svelte';

describe('player store', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('play() fetches the session and populates current', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					session_id: 's1',
					start_position: 0,
					tracks: [{ id: 1, position: 1, title: 'T', duration: 100 }],
					chapters: []
				})
			})
		);
		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.sessionId).toBe('s1');
		expect(store.current?.tracks).toHaveLength(1);
	});

	it('pause()/resume() toggle `playing` without a network call', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					session_id: 's1',
					start_position: 0,
					tracks: [{ id: 1, position: 1, title: 'T', duration: 100 }],
					chapters: []
				})
			})
		);
		const store = createPlayerStore();
		await store.play(42);
		store.pause();
		expect(store.current?.playing).toBe(false);
		store.resume();
		expect(store.current?.playing).toBe(true);
	});

	it('play() resuming past the first track lands on the correct trackIndex, not always 0', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					session_id: 's1',
					start_position: 150, // 100s into track 2 (track 1 is 100s long)
					tracks: [
						{ id: 1, position: 1, title: 'T1', duration: 100 },
						{ id: 2, position: 2, title: 'T2', duration: 100 }
					],
					chapters: []
				})
			})
		);
		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.trackIndex).toBe(1);
	});

	it('attachAudioElement() stops the previous element before adopting a new one', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					session_id: 's1',
					start_position: 0,
					tracks: [{ id: 1, position: 1, title: 'T', duration: 100 }],
					chapters: []
				})
			})
		);

		const store = createPlayerStore();
		await store.play(42);

		// Create two mock audio elements with HTMLAudioElement-like shape
		const createMockAudioElement = () => ({
			pause: vi.fn(),
			removeAttribute: vi.fn(),
			play: vi.fn(),
			src: '',
			currentTime: 0,
			ontimeupdate: null as unknown,
			onended: null as unknown
		});

		const oldElement = createMockAudioElement();
		const newElement = createMockAudioElement();

		// Attach the first element
		store.attachAudioElement(oldElement as unknown as HTMLAudioElement);
		expect(oldElement.pause).not.toHaveBeenCalled();

		// Attach the second element; the first should be stopped
		store.attachAudioElement(newElement as unknown as HTMLAudioElement);
		expect(oldElement.pause).toHaveBeenCalled();
		expect(oldElement.removeAttribute).toHaveBeenCalledWith('src');
	});

	it('reloadCurrentTrack() re-reads current.tracks[trackIndex] even when trackIndex is unchanged (shuffle case)', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					session_id: 's1',
					start_position: 0,
					tracks: [
						{ id: 1, position: 1, title: 'T1', duration: 100 },
						{ id: 2, position: 2, title: 'T2', duration: 100 }
					],
					chapters: []
				})
			})
		);

		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.trackIndex).toBe(0);

		const fakeAudioElement = {
			pause: vi.fn(),
			removeAttribute: vi.fn(),
			play: vi.fn(),
			src: '',
			currentTime: 0,
			ontimeupdate: null as unknown,
			onended: null as unknown
		};
		store.attachAudioElement(fakeAudioElement as unknown as HTMLAudioElement);
		expect(fakeAudioElement.src).toBe('/tracks/1/stream');

		// Simulate a shuffle: swap the two tracks in place. trackIndex stays 0,
		// but the track that actually sits at index 0 has changed.
		if (store.current) {
			const tracks = store.current.tracks;
			[tracks[0], tracks[1]] = [tracks[1], tracks[0]];
		}
		expect(store.current?.trackIndex).toBe(0); // index itself is unchanged

		store.reloadCurrentTrack();

		// Proves the reload actually re-read current.tracks[trackIndex]: the
		// audio element must now point at track 2 (id 2), which is what a
		// naive seek(0) — same trackIndex as before — would have failed to do.
		expect(fakeAudioElement.src).toBe('/tracks/2/stream');
	});
	/** Three tracks of 100s each, so a track index maps to a round position. */
	function stubSession(startPosition = 0) {
		const fetchMock = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				session_id: 's1',
				kind: 'album',
				has_cover: true,
				start_position: startPosition,
				tracks: [
					{ id: 11, position: 1, title: 'T1', duration: 100 },
					{ id: 22, position: 2, title: 'T2', duration: 100 },
					{ id: 33, position: 3, title: 'T3', duration: 100 }
				],
				chapters: []
			})
		});
		vi.stubGlobal('fetch', fetchMock);
		return fetchMock;
	}

	it('playTrackAt() starts a not-yet-loaded item at the requested track', async () => {
		stubSession(250); // saved progress sits in track 3
		const store = createPlayerStore();
		await store.playTrackAt(42, 1);
		expect(store.current?.trackIndex).toBe(1);
		// The requested track wins over the resume position, otherwise clicking
		// row 2 would drop the listener wherever they last stopped.
		expect(store.current?.position).toBe(100);
		expect(store.current?.playing).toBe(true);
	});

	it('playTrackAt() jumps inside an already-loaded item without a second session', async () => {
		const fetchMock = stubSession();
		const store = createPlayerStore();
		await store.play(42);
		expect(fetchMock).toHaveBeenCalledTimes(1);

		await store.playTrackAt(42, 2);
		expect(store.current?.trackIndex).toBe(2);
		expect(store.current?.position).toBe(200);
		// Still one /play call: re-POSTing would open a second playback session
		// for something already playing.
		expect(fetchMock).toHaveBeenCalledTimes(1);
	});

	it('playTrackAt() clamps an out-of-range index instead of loading nothing', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		await store.playTrackAt(42, 99);
		expect(store.current?.trackIndex).toBe(2);
	});

	it('playTrackById() resolves the track by id, not by running order', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.playTrackById(42, 33);
		expect(store.current?.trackIndex).toBe(2);
		expect(store.current?.position).toBe(200);
	});

	it('playTrackById() ignores a track id the item does not contain', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		await store.playTrackById(42, 999);
		expect(store.current?.trackIndex).toBe(0); // unchanged
	});

	it('playFrom() seeks to an absolute position across a track boundary', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.playFrom(42, 250); // 50s into track 3 — a chapter or bookmark
		expect(store.current?.trackIndex).toBe(2);
		expect(store.current?.position).toBe(250);
		expect(store.current?.playing).toBe(true);
	});
	it('nextTrack() advances and stops at the last track', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		store.nextTrack();
		expect(store.current?.trackIndex).toBe(1);
		store.nextTrack();
		expect(store.current?.trackIndex).toBe(2);
		store.nextTrack();
		expect(store.current?.trackIndex).toBe(2);
	});

	it('previousTrack() restarts the track when more than 3s in', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		store.nextTrack();
		store.seek(150);
		expect(store.trackOffset()).toBe(50);
		store.previousTrack();
		expect(store.current?.trackIndex).toBe(1);
		expect(store.current?.position).toBe(100);
	});

	it('previousTrack() steps back when near the start of the track', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		store.nextTrack();
		store.seek(102);
		store.previousTrack();
		expect(store.current?.trackIndex).toBe(0);
		expect(store.current?.position).toBe(0);
	});

	it('previousTrack() on the first track restarts it rather than underflowing', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		store.seek(1);
		store.previousTrack();
		expect(store.current?.trackIndex).toBe(0);
		expect(store.current?.position).toBe(0);
	});

	it('trackOffset() is relative to the current track, not the whole item', async () => {
		stubSession(250);
		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.trackIndex).toBe(2);
		expect(store.trackOffset()).toBe(50);
	});

	it('play() records the item kind so the UI can branch on it', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.kind).toBe('album');
	});
	it('play() records whether the item has a cover so the bar can show it', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.hasCover).toBe(true);
	});

	it('hasCover is false when the response omits it', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					session_id: 's1',
					start_position: 0,
					tracks: [{ id: 1, position: 1, title: 'T', duration: 100 }],
					chapters: []
				})
			})
		);
		const store = createPlayerStore();
		await store.play(42);
		expect(store.current?.hasCover).toBe(false);
	});
	it('seekInTrack() is relative to the current track, not the whole item', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		store.nextTrack();
		store.seekInTrack(40);
		expect(store.current?.position).toBe(140);
		expect(store.trackOffset()).toBe(40);
		expect(store.current?.trackIndex).toBe(1);
	});

	it('seekInTrack() on the first track matches an absolute seek', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);
		store.seekInTrack(25);
		expect(store.current?.position).toBe(25);
	});
	it('applyPreferences() called from an effect does not re-trigger itself', async () => {
		stubSession();
		const store = createPlayerStore();
		await store.play(42);

		let runs = 0;
		const cleanup = $effect.root(() => {
			$effect(() => {
				runs += 1;
				store.applyPreferences({ playbackSpeed: 1.5, skipBack: 20, skipForward: 20 });
			});
		});
		await new Promise((resolve) => setTimeout(resolve, 50));
		cleanup();

		expect(runs).toBe(1);
		expect(store.preferences.playbackSpeed).toBe(1.5);
		expect(store.current?.speed).toBe(1.5);
	});

	it('takes its starting preferences from the caller', async () => {
		stubSession();
		const store = createPlayerStore({ playbackSpeed: 2, skipBack: 45, skipForward: 5 });
		expect(store.preferences.skipBack).toBe(45);
		await store.play(42);
		expect(store.current?.speed).toBe(2);
	});
	it('new preferences reach the running player without restarting it', async () => {
		stubSession();
		const store = createPlayerStore({ playbackSpeed: 1, skipBack: 30, skipForward: 15 });
		await store.play(42);
		store.seek(200);
		const session = store.current?.sessionId;

		store.applyPreferences({ playbackSpeed: 1.25, skipBack: 45, skipForward: 5 });

		store.skipBack();
		expect(store.current?.position).toBe(155);
		store.skipForward();
		expect(store.current?.position).toBe(160);
		expect(store.current?.speed).toBe(1.25);
		expect(store.current?.sessionId).toBe(session);
	});
	it('getAnalyser() taps the source without taking the audio path with it', async () => {
		const store = createPlayerStore();
		const el = document.createElement('audio');
		document.body.appendChild(el);
		store.attachAudioElement(el);

		const node = store.getAnalyser();
		expect(node).not.toBeNull();
		expect(node?.frequencyBinCount).toBeGreaterThan(0);
		expect(store.getAnalyser()).toBe(node);
		el.remove();
	});

	it('getAnalyser() gives up quietly when there is no element yet', () => {
		const store = createPlayerStore();
		expect(store.getAnalyser()).toBeNull();
	});

	function sessionFetch(startPosition = 0) {
		return vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				session_id: 's1',
				start_position: startPosition,
				tracks: [
					{ id: 1, position: 1, title: 'T1', duration: 100 },
					{ id: 2, position: 2, title: 'T2', duration: 100 }
				],
				chapters: []
			})
		});
	}

	function fakeAudio(play: () => unknown) {
		return {
			pause: vi.fn(),
			removeAttribute: vi.fn(),
			play,
			src: '',
			currentTime: 0,
			readyState: 0,
			playbackRate: 1,
			onplay: null as unknown,
			onpause: null as unknown,
			onloadedmetadata: null as (() => void) | null,
			ontimeupdate: null as unknown,
			onended: null as unknown
		};
	}

	it('a refused autoplay leaves the player on pause instead of claiming to play', async () => {
		vi.stubGlobal('fetch', sessionFetch());
		const store = createPlayerStore();
		await store.play(42);
		const refusal = Object.assign(new Error('blocked'), { name: 'NotAllowedError' });
		const el = fakeAudio(() => Promise.reject(refusal));
		store.attachAudioElement(el as unknown as HTMLAudioElement);
		await vi.waitFor(() => expect(store.current?.playing).toBe(false));
	});

	it('a pause coming from the element itself is believed', async () => {
		vi.stubGlobal('fetch', sessionFetch());
		const store = createPlayerStore();
		await store.play(42);
		const el = fakeAudio(() => Promise.resolve());
		store.attachAudioElement(el as unknown as HTMLAudioElement);
		await vi.waitFor(() => expect(store.current?.playing).toBe(true));

		(el.onpause as () => void)();
		expect(store.current?.playing).toBe(false);
		(el.onplay as () => void)();
		expect(store.current?.playing).toBe(true);
	});

	it('the pause that a source swap fires does not count as a stop', async () => {
		vi.stubGlobal('fetch', sessionFetch());
		const store = createPlayerStore();
		await store.play(42);
		// Never settles: the element is still working its way into playback.
		const el = fakeAudio(() => new Promise<void>(() => {}));
		store.attachAudioElement(el as unknown as HTMLAudioElement);

		(el.onpause as () => void)();
		expect(store.current?.playing).toBe(true);
	});

	it('a position set before the metadata arrives is written again once it does', async () => {
		vi.stubGlobal('fetch', sessionFetch(150));
		const store = createPlayerStore();
		await store.play(42);
		const el = fakeAudio(() => Promise.resolve());
		store.attachAudioElement(el as unknown as HTMLAudioElement);

		// readyState 0: the browser drops the write, so the store must repeat it.
		expect(el.currentTime).toBe(0);
		el.readyState = 1;
		el.onloadedmetadata?.();
		expect(el.currentTime).toBe(50);
	});

	it('a second call after an element swap does not reuse the old graph', async () => {
		const store = createPlayerStore();
		const first = document.createElement('audio');
		const second = document.createElement('audio');
		document.body.append(first, second);

		store.attachAudioElement(first);
		const one = store.getAnalyser();
		store.attachAudioElement(second);
		const two = store.getAnalyser();

		expect(one).not.toBeNull();
		expect(two).not.toBeNull();
		expect(two).not.toBe(one);
		first.remove();
		second.remove();
	});
});
