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
});
