export interface PlayerTrack {
	id: number;
	position: number;
	title: string | null;
	duration: number;
}
export interface PlayerState {
	itemId: number;
	sessionId: string;
	tracks: PlayerTrack[];
	chapters: { title: string; start: number; end: number }[];
	trackIndex: number;
	position: number; // seconds, absolute across all tracks of the item
	playing: boolean;
	speed: number;
}

function trackStart(tracks: PlayerTrack[], trackIndex: number): number {
	let sum = 0;
	for (let i = 0; i < trackIndex; i++) sum += tracks[i].duration;
	return sum;
}

/** Maps an absolute item-position to the track it falls in and the offset
 * within that track. Clamps to the last track if the position exceeds the
 * item's total duration (skipForward can overshoot near the end). */
function locate(
	tracks: PlayerTrack[],
	absolutePosition: number
): { trackIndex: number; offset: number } {
	let elapsed = 0;
	for (let i = 0; i < tracks.length; i++) {
		const end = elapsed + tracks[i].duration;
		if (absolutePosition < end || i === tracks.length - 1) {
			return { trackIndex: i, offset: Math.max(0, absolutePosition - elapsed) };
		}
		elapsed = end;
	}
	return { trackIndex: 0, offset: 0 };
}

function clampIndex(tracks: PlayerTrack[], index: number): number {
	return Math.max(0, Math.min(index, tracks.length - 1));
}

export function createPlayerStore() {
	let current = $state<PlayerState | null>(null);
	let heartbeat: ReturnType<typeof setInterval> | null = null;
	let audioEl: HTMLAudioElement | null = null;

	function stopHeartbeat() {
		if (heartbeat) clearInterval(heartbeat);
		heartbeat = null;
	}

	function startHeartbeat() {
		stopHeartbeat();
		heartbeat = setInterval(() => {
			if (current && current.playing) void flushProgress(false);
		}, 15_000);
	}

	async function flushProgress(finished: boolean) {
		if (!current) return;
		await fetch(`/progress/${current.itemId}`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position: current.position, finished })
		});
	}

	/** Points the shared <audio> element at the given track/offset and, if
	 * the store says we're playing, starts it. Called on initial play(), on
	 * auto-advance (onended), and on any seek/skip that crosses a track
	 * boundary. */
	function loadTrack(trackIndex: number, offset: number) {
		if (!current) return;
		const track = current.tracks[trackIndex];
		if (!track) return;
		// State first, media second. The old order bailed out on a missing
		// audio element before assigning trackIndex, so any jump made before
		// MiniPlayerBar mounted left the store pointing at the wrong track
		// while the UI happily rendered it as current.
		current.trackIndex = trackIndex;
		if (!audioEl) return;
		audioEl.src = `/tracks/${track.id}/stream`;
		audioEl.currentTime = offset;
		if (current.playing) void audioEl.play();
	}

	/** Called once, from MiniPlayerBar's onMount — hands the store direct,
	 * imperative control of the one persistent <audio> element. A single
	 * owner (this) avoids the feedback loop a two-way reactive binding would
	 * create between `ontimeupdate` (audio → state) and a $effect that tries
	 * to write that same time back (state → audio). */
	function attachAudioElement(el: HTMLAudioElement) {
		if (audioEl && audioEl !== el) {
			audioEl.pause();
			audioEl.removeAttribute('src');
		}
		audioEl = el;
		audioEl.ontimeupdate = () => {
			if (!current || !audioEl) return;
			current.position = trackStart(current.tracks, current.trackIndex) + audioEl.currentTime;
		};
		audioEl.onended = () => {
			if (!current) return;
			const nextIndex = current.trackIndex + 1;
			if (nextIndex < current.tracks.length) {
				loadTrack(nextIndex, 0);
			} else {
				current.playing = false;
				void flushProgress(true);
			}
		};
		if (current) {
			const { trackIndex, offset } = locate(current.tracks, current.position);
			loadTrack(trackIndex, offset);
		}
	}

	/** `startTrackIndex` starts the item at a given track instead of where the
	 * listener left off — what clicking a row in a track list means, as opposed
	 * to the big play button's "carry on where I was". */
	async function play(itemId: number, startTrackIndex?: number) {
		const res = await fetch(`/play/${itemId}`, { method: 'POST' });
		const body = await res.json();
		const tracks: PlayerTrack[] = body.tracks;

		let trackIndex: number;
		let offset: number;
		let position: number;
		if (startTrackIndex === undefined) {
			({ trackIndex, offset } = locate(tracks, body.start_position));
			position = body.start_position;
		} else {
			trackIndex = clampIndex(tracks, startTrackIndex);
			offset = 0;
			position = trackStart(tracks, trackIndex);
		}

		current = {
			itemId,
			sessionId: body.session_id,
			tracks,
			chapters: body.chapters,
			trackIndex,
			position,
			playing: true,
			speed: 1
		};
		startHeartbeat();
		loadTrack(trackIndex, offset);
	}

	/** Moves the already-loaded item to the start of one of its tracks. */
	function jumpToTrack(trackIndex: number) {
		if (!current) return;
		const index = clampIndex(current.tracks, trackIndex);
		current.playing = true;
		current.position = trackStart(current.tracks, index);
		loadTrack(index, 0);
	}

	/** Plays one specific track of an item. When that item is already loaded it
	 * jumps within it instead of POSTing /play again, which would open a second
	 * playback session for something already playing. */
	async function playTrackAt(itemId: number, trackIndex: number) {
		if (!current || current.itemId !== itemId) {
			await play(itemId, trackIndex);
			return;
		}
		jumpToTrack(trackIndex);
	}

	/** Same, but identifying the track by id — for callers that know which
	 * track they mean but not where it sits in its item's running order, such
	 * as search results. */
	async function playTrackById(itemId: number, trackId: number) {
		if (!current || current.itemId !== itemId) await play(itemId);
		const index = current?.tracks.findIndex((t) => t.id === trackId) ?? -1;
		if (index < 0) return;
		jumpToTrack(index);
	}

	/** Plays an item from an absolute position — what selecting a chapter or a
	 * bookmark means. Loads the item first when something else is playing. */
	async function playFrom(itemId: number, position: number) {
		if (!current || current.itemId !== itemId) await play(itemId);
		if (!current) return;
		current.playing = true;
		seek(position);
	}

	function pause() {
		if (!current) return;
		current.playing = false;
		audioEl?.pause();
	}
	function resume() {
		if (!current) return;
		current.playing = true;
		void audioEl?.play();
	}
	function seek(seconds: number) {
		if (!current) return;
		const clamped = Math.max(0, seconds);
		const { trackIndex, offset } = locate(current.tracks, clamped);
		if (trackIndex !== current.trackIndex) {
			loadTrack(trackIndex, offset);
		} else if (audioEl) {
			audioEl.currentTime = offset;
		}
		current.position = clamped;
	}
	function skipBack(seconds: number) {
		if (current) seek(current.position - seconds);
	}
	function skipForward(seconds: number) {
		if (current) seek(current.position + seconds);
	}

	/** Reloads the <audio> element for whatever track currently sits at
	 * `current.trackIndex`, even if that index number hasn't changed — needed
	 * when the *contents* of `current.tracks` were reordered externally (e.g.
	 * a shuffle), which `seek()`'s index-comparison optimization can't detect
	 * on its own since the index itself is unchanged. */
	function reloadCurrentTrack() {
		if (!current) return;
		loadTrack(current.trackIndex, 0);
	}

	async function close() {
		if (!current) return;
		await flushProgress(false);
		await fetch(`/sessions/${current.sessionId}/close`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position: current.position, duration: current.position })
		});
		stopHeartbeat();
		if (audioEl) {
			audioEl.pause();
			audioEl.removeAttribute('src');
		}
		current = null;
	}

	return {
		get current() {
			return current;
		},
		play,
		playTrackAt,
		playTrackById,
		playFrom,
		pause,
		resume,
		seek,
		skipBack,
		skipForward,
		close,
		attachAudioElement,
		reloadCurrentTrack
	};
}

export type PlayerStore = ReturnType<typeof createPlayerStore>;
export const PLAYER_CONTEXT_KEY = Symbol('player');
