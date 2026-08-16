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
		if (!current || !audioEl) return;
		const track = current.tracks[trackIndex];
		if (!track) return;
		current.trackIndex = trackIndex;
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

	async function play(itemId: number) {
		const res = await fetch(`/play/${itemId}`, { method: 'POST' });
		const body = await res.json();
		const tracks: PlayerTrack[] = body.tracks;
		const { trackIndex, offset } = locate(tracks, body.start_position);
		current = {
			itemId,
			sessionId: body.session_id,
			tracks,
			chapters: body.chapters,
			trackIndex,
			position: body.start_position,
			playing: true,
			speed: 1
		};
		startHeartbeat();
		loadTrack(trackIndex, offset);
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
		pause,
		resume,
		seek,
		skipBack,
		skipForward,
		close,
		attachAudioElement
	};
}

export type PlayerStore = ReturnType<typeof createPlayerStore>;
export const PLAYER_CONTEXT_KEY = Symbol('player');
