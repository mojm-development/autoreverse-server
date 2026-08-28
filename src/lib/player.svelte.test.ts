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
});
