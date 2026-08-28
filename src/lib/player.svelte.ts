export interface PlayerTrack {
	id: number;
	position: number;
	title: string | null;
	duration: number;
}
export interface PlayerState {
	itemId: number;
	kind: string;
	hasCover: boolean;
	sessionId: string;
	tracks: PlayerTrack[];
	chapters: { title: string; start: number; end: number }[];
	trackIndex: number;
	position: number;
	playing: boolean;
	speed: number;
}

function trackStart(tracks: PlayerTrack[], trackIndex: number): number {
	let sum = 0;
	for (let i = 0; i < trackIndex; i++) sum += tracks[i].duration;
	return sum;
}

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

	function loadTrack(trackIndex: number, offset: number) {
		if (!current) return;
		const track = current.tracks[trackIndex];
		if (!track) return;
		current.trackIndex = trackIndex;
		if (!audioEl) return;
		audioEl.src = `/tracks/${track.id}/stream`;
		audioEl.currentTime = offset;
		if (current.playing) void audioEl.play();
	}

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
			kind: body.kind ?? 'album',
			hasCover: Boolean(body.has_cover),
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

	function jumpToTrack(trackIndex: number) {
		if (!current) return;
		const index = clampIndex(current.tracks, trackIndex);
		current.playing = true;
		current.position = trackStart(current.tracks, index);
		loadTrack(index, 0);
	}

	async function playTrackAt(itemId: number, trackIndex: number) {
		if (!current || current.itemId !== itemId) {
			await play(itemId, trackIndex);
			return;
		}
		jumpToTrack(trackIndex);
	}

	async function playTrackById(itemId: number, trackId: number) {
		if (!current || current.itemId !== itemId) await play(itemId);
		const index = current?.tracks.findIndex((t) => t.id === trackId) ?? -1;
		if (index < 0) return;
		jumpToTrack(index);
	}

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

	function seekInTrack(offset: number) {
		if (!current) return;
		seek(trackStart(current.tracks, current.trackIndex) + offset);
	}

	function trackOffset(): number {
		if (!current) return 0;
		return current.position - trackStart(current.tracks, current.trackIndex);
	}

	function nextTrack() {
		if (!current) return;
		if (current.trackIndex >= current.tracks.length - 1) return;
		jumpToTrack(current.trackIndex + 1);
	}

	function previousTrack() {
		if (!current) return;
		if (trackOffset() > 3 || current.trackIndex === 0) {
			jumpToTrack(current.trackIndex);
			return;
		}
		jumpToTrack(current.trackIndex - 1);
	}

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
		nextTrack,
		previousTrack,
		seekInTrack,
		trackOffset,
		close,
		attachAudioElement,
		reloadCurrentTrack
	};
}

export type PlayerStore = ReturnType<typeof createPlayerStore>;
export const PLAYER_CONTEXT_KEY = Symbol('player');
