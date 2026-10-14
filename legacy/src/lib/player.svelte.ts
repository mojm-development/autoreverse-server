import { untrack } from 'svelte';
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

export interface PlayerPreferences {
	playbackSpeed: number;
	skipBack: number;
	skipForward: number;
}

const DEFAULT_PREFERENCES: PlayerPreferences = {
	playbackSpeed: 1,
	skipBack: 30,
	skipForward: 15
};

export function createPlayerStore(initial?: Partial<PlayerPreferences>) {
	let current = $state<PlayerState | null>(null);
	let preferences = $state<PlayerPreferences>({ ...DEFAULT_PREFERENCES, ...initial });
	let audioContext: AudioContext | null = null;
	let analyser: AnalyserNode | null = null;
	let analyserUnavailable = false;
	let heartbeat: ReturnType<typeof setInterval> | null = null;
	let audioEl: HTMLAudioElement | null = null;
	let pendingOffset: number | null = null;
	let switchingTrack = false;

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

	/**
	 * A fresh `src` has no duration yet, and `currentTime` written before the
	 * metadata arrives is dropped on the floor — remember it and write it again
	 * from `onloadedmetadata`.
	 */
	function setOffset(offset: number) {
		if (!audioEl) return;
		pendingOffset = offset;
		if (audioEl.readyState >= 1) {
			audioEl.currentTime = offset;
			pendingOffset = null;
		}
	}

	function startPlayback() {
		if (!audioEl) return;
		void audioContext?.resume().catch(() => undefined);
		const started = audioEl.play() as Promise<void> | undefined;
		if (!started?.catch) {
			switchingTrack = false;
			return;
		}
		started
			.then(() => {
				switchingTrack = false;
			})
			.catch((error: unknown) => {
				switchingTrack = false;
				// A reload carries no user gesture, so the browser refuses to play. Say so
				// instead of showing a pause button over silence.
				const name = (error as { name?: string } | null)?.name;
				if (current && (name === 'NotAllowedError' || name === 'NotSupportedError')) {
					current.playing = false;
				}
			});
	}

	function loadTrack(trackIndex: number, offset: number) {
		if (!current) return;
		const track = current.tracks[trackIndex];
		if (!track) return;
		current.trackIndex = trackIndex;
		if (!audioEl) return;
		// Reloading the source fires `pause` on an element that was playing; that one is
		// bookkeeping, not the user, so it must not flip `playing`.
		switchingTrack = true;
		audioEl.src = `/tracks/${track.id}/stream`;
		setOffset(offset);
		audioEl.playbackRate = current.speed;
		if (current.playing) startPlayback();
		else switchingTrack = false;
	}

	function releaseAnalyser() {
		analyser = null;
		analyserUnavailable = false;
		const context = audioContext;
		audioContext = null;
		if (context) void context.close().catch(() => undefined);
	}

	function getAnalyser(): AnalyserNode | null {
		if (analyser || analyserUnavailable || !audioEl) return analyser;
		try {
			const Constructor = window.AudioContext;
			if (!Constructor) {
				analyserUnavailable = true;
				return null;
			}
			const context = new Constructor();
			const source = context.createMediaElementSource(audioEl);
			source.connect(context.destination);
			const node = context.createAnalyser();
			// 2048 buys ~21 Hz bins, enough resolution for a log-spaced band split in the
			// visualizer; the default -30 dB ceiling clips loud music into a solid block, so
			// give the top end headroom and lift the floor above the room noise.
			node.fftSize = 2048;
			node.smoothingTimeConstant = 0.65;
			node.minDecibels = -62;
			node.maxDecibels = -8;
			source.connect(node);
			audioContext = context;
			analyser = node;
			void context.resume().catch(() => undefined);
			return analyser;
		} catch {
			analyserUnavailable = true;
			return null;
		}
	}

	function attachAudioElement(el: HTMLAudioElement) {
		if (audioEl && audioEl !== el) {
			audioEl.pause();
			audioEl.removeAttribute('src');
			releaseAnalyser();
		}
		audioEl = el;
		// The element is the truth about what is audible: media keys, headset buttons and a
		// refused autoplay all change it without going through this store.
		audioEl.onplay = () => {
			if (current) current.playing = true;
		};
		audioEl.onpause = () => {
			if (current && !switchingTrack) current.playing = false;
		};
		audioEl.onloadedmetadata = () => {
			if (pendingOffset === null || !audioEl) return;
			audioEl.currentTime = pendingOffset;
			pendingOffset = null;
		};
		audioEl.ontimeupdate = () => {
			if (!current || !audioEl) return;
			current.position = trackStart(current.tracks, current.trackIndex) + audioEl.currentTime;
		};
		audioEl.onended = () => {
			if (!current) return;
			const nextIndex = current.trackIndex + 1;
			if (nextIndex < current.tracks.length) {
				// Running out of media fires `pause` *before* `ended`, so `playing` is already
				// false by now — that pause is the track ending, not the user. jumpToTrack puts
				// it back and starts the next track, the same path the next button takes.
				jumpToTrack(nextIndex);
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
			speed: preferences.playbackSpeed
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
		if (audioEl && !audioEl.src) {
			// Nothing loaded yet (a reload rebuilt the element): pick the track back up.
			const { trackIndex, offset } = locate(current.tracks, current.position);
			loadTrack(trackIndex, offset);
			return;
		}
		startPlayback();
	}
	function seek(seconds: number) {
		if (!current) return;
		const clamped = Math.max(0, seconds);
		const { trackIndex, offset } = locate(current.tracks, clamped);
		if (trackIndex !== current.trackIndex) {
			loadTrack(trackIndex, offset);
		} else if (audioEl) {
			setOffset(offset);
		}
		current.position = clamped;
	}
	function skipBack(seconds = preferences.skipBack) {
		if (current) seek(current.position - seconds);
	}
	function skipForward(seconds = preferences.skipForward) {
		if (current) seek(current.position + seconds);
	}

	function setSpeed(value: number) {
		const clamped = Math.min(4, Math.max(0.5, value));
		preferences = { ...preferences, playbackSpeed: clamped };
		if (current) current.speed = clamped;
		if (audioEl) audioEl.playbackRate = clamped;
	}

	function applyPreferences(next: Partial<PlayerPreferences>) {
		untrack(() => {
			preferences = { ...preferences, ...next };
			if (current) current.speed = preferences.playbackSpeed;
			if (audioEl) audioEl.playbackRate = preferences.playbackSpeed;
		});
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

	/** Album: 10 s, the way music players step. Book or podcast: the user's own values. */
	function nudgeStep(direction: -1 | 1): number {
		if (current?.kind === 'album') return 10;
		return direction < 0 ? preferences.skipBack : preferences.skipForward;
	}

	function nudge(direction: -1 | 1) {
		if (!current) return;
		const step = nudgeStep(direction) * direction;
		if (current.kind === 'album') seekInTrack(Math.max(0, trackOffset() + step));
		else seek(current.position + step);
	}

	function chapterIndexAt(position: number): number {
		if (!current) return -1;
		return current.chapters.findIndex((c) => position >= c.start && position < c.end);
	}

	/**
	 * One step back: the track for an album, the chapter otherwise — and, past three
	 * seconds in, the start of the current one first, like every other player.
	 */
	function jumpPrevious() {
		if (!current) return;
		if (current.kind === 'album' || current.chapters.length === 0) {
			previousTrack();
			return;
		}
		const index = chapterIndexAt(current.position);
		const chapter = current.chapters[index];
		if (chapter && current.position - chapter.start > 3) {
			seek(chapter.start);
			return;
		}
		const previous = current.chapters[index - 1];
		if (previous) seek(previous.start);
		else if (chapter) seek(chapter.start);
	}

	function jumpNext() {
		if (!current) return;
		if (current.kind === 'album' || current.chapters.length === 0) {
			nextTrack();
			return;
		}
		const next = current.chapters[chapterIndexAt(current.position) + 1];
		if (next) seek(next.start);
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
		get preferences() {
			return preferences;
		},
		setSpeed,
		applyPreferences,
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
		nudge,
		nudgeStep,
		jumpPrevious,
		jumpNext,
		seekInTrack,
		trackOffset,
		close,
		getAnalyser,
		attachAudioElement,
		reloadCurrentTrack
	};
}

export type PlayerStore = ReturnType<typeof createPlayerStore>;
export const PLAYER_CONTEXT_KEY = Symbol('player');
