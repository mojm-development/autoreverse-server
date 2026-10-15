// The web player's state and transport (legacy src/lib/player.svelte.ts), without a framework:
// the store mutates `current` and tells its subscribers. One store per page load; it owns the
// one <audio> element, which lives outside the swapped part of the page and so keeps playing
// across boosted navigations.

/** Where track `trackIndex` starts within the whole item, in seconds. */
export function trackStart(tracks, trackIndex) {
	let sum = 0;
	for (let i = 0; i < trackIndex; i++) sum += tracks[i].duration;
	return sum;
}

/** The track an absolute position falls into, and the offset inside it. */
export function locate(tracks, absolutePosition) {
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

export function clampIndex(tracks, index) {
	return Math.max(0, Math.min(index, tracks.length - 1));
}

export function totalDuration(tracks) {
	return tracks.reduce((sum, t) => sum + t.duration, 0);
}

/** Index of the chapter playing at `position`, -1 between or outside chapters. */
export function chapterIndexAt(chapters, position) {
	return chapters.findIndex((c) => position >= c.start && position < c.end);
}

/** Fisher–Yates, in place. */
export function shuffleInPlace(list, random = Math.random) {
	for (let i = list.length - 1; i > 0; i--) {
		const j = Math.floor(random() * (i + 1));
		[list[i], list[j]] = [list[j], list[i]];
	}
	return list;
}

export const DEFAULT_PREFERENCES = { playbackSpeed: 1, skipBack: 30, skipForward: 15 };
const HEARTBEAT_MS = 15_000;

/**
 * @param {object} [options]
 * @param {Partial<typeof DEFAULT_PREFERENCES>} [options.preferences]
 * @param {typeof fetch} [options.fetch]
 * @param {() => AudioContext | null} [options.createAudioContext]
 * @param {(ended: { itemId: number, trackId: number, last: boolean }) => boolean} [options.onEnded]
 *   Called whenever a track runs out; returning true means the caller plays on itself (a
 *   playlist), so the store does not continue with the item's next track.
 */
export function createPlayerStore(options = {}) {
	const request = options.fetch ?? ((...args) => fetch(...args));
	const onEnded = options.onEnded;
	const createAudioContext =
		options.createAudioContext ??
		(() => (globalThis.AudioContext ? new globalThis.AudioContext() : null));

	let current = null;
	let preferences = { ...DEFAULT_PREFERENCES, ...options.preferences };
	let audioContext = null;
	let analyser = null;
	let analyserUnavailable = false;
	let heartbeat = null;
	let audioEl = null;
	let pendingOffset = null;
	let switchingTrack = false;
	const listeners = new Set();

	function notify() {
		for (const listener of listeners) listener();
	}

	function subscribe(listener) {
		listeners.add(listener);
		return () => listeners.delete(listener);
	}

	function stopHeartbeat() {
		if (heartbeat) clearInterval(heartbeat);
		heartbeat = null;
	}

	function startHeartbeat() {
		stopHeartbeat();
		heartbeat = setInterval(() => {
			if (current && current.playing) void flushProgress(false);
		}, HEARTBEAT_MS);
		// Outside a browser (the tests) the timer must not keep the process alive.
		heartbeat.unref?.();
	}

	async function flushProgress(finished) {
		if (!current) return;
		try {
			await request(`/progress/${current.itemId}`, {
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ position: current.position, finished })
			});
		} catch {
			// Offline for a moment: the next heartbeat tries again.
		}
	}

	/** Ends a session on the server and books its listening time; the player carries on. */
	function closeSession(state) {
		void request(`/sessions/${state.sessionId}/close`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position: state.position, duration: state.position })
		}).catch(() => undefined);
	}

	/**
	 * A fresh `src` has no duration yet, and `currentTime` written before the metadata arrives
	 * is dropped on the floor — remember it and write it again from `loadedmetadata`.
	 */
	function setOffset(offset) {
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
		const started = audioEl.play();
		if (!started?.catch) {
			switchingTrack = false;
			return;
		}
		started
			.then(() => {
				switchingTrack = false;
			})
			.catch((error) => {
				switchingTrack = false;
				// Without a user gesture the browser refuses to play. Say so instead of showing a
				// pause button over silence.
				const name = error?.name;
				if (current && (name === 'NotAllowedError' || name === 'NotSupportedError')) {
					current.playing = false;
					notify();
				}
			});
	}

	function loadTrack(trackIndex, offset) {
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

	function getAnalyser() {
		if (analyser || analyserUnavailable || !audioEl) return analyser;
		try {
			const context = createAudioContext();
			if (!context) {
				analyserUnavailable = true;
				return null;
			}
			const source = context.createMediaElementSource(audioEl);
			source.connect(context.destination);
			const node = context.createAnalyser();
			// 2048 buys ~21 Hz bins, enough for a log-spaced band split; the default -30 dB
			// ceiling clips loud music into a solid block, so give the top end headroom and lift
			// the floor above the room noise.
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

	function attachAudioElement(el) {
		if (audioEl && audioEl !== el) {
			audioEl.pause();
			audioEl.removeAttribute('src');
			releaseAnalyser();
		}
		audioEl = el;
		// The element is the truth about what is audible: media keys, headset buttons and a
		// refused autoplay all change it without going through this store.
		audioEl.onplay = () => {
			if (current) {
				current.playing = true;
				notify();
			}
		};
		audioEl.onpause = () => {
			if (current && !switchingTrack) {
				current.playing = false;
				notify();
			}
		};
		audioEl.onloadedmetadata = () => {
			if (pendingOffset === null || !audioEl) return;
			audioEl.currentTime = pendingOffset;
			pendingOffset = null;
		};
		audioEl.ontimeupdate = () => {
			if (!current || !audioEl) return;
			current.position = trackStart(current.tracks, current.trackIndex) + audioEl.currentTime;
			notify();
		};
		audioEl.onended = () => {
			if (!current) return;
			const nextIndex = current.trackIndex + 1;
			const last = nextIndex >= current.tracks.length;
			if (last) {
				current.playing = false;
				void flushProgress(true);
				notify();
			}
			// A queue (the playlist page) may take over: next entry instead of the next track.
			const handled = onEnded?.({
				itemId: current.itemId,
				trackId: current.tracks[current.trackIndex]?.id,
				last
			});
			// Running out of media fires `pause` before `ended`; that pause is the track
			// ending, not the user. jumpToTrack puts `playing` back and starts the next one.
			if (!handled && !last) jumpToTrack(nextIndex);
		};
		if (current) {
			const { trackIndex, offset } = locate(current.tracks, current.position);
			loadTrack(trackIndex, offset);
		}
	}

	/**
	 * Opens a session for `itemId` and starts it: at the saved position, or at the start of
	 * track `startTrackIndex`. `shuffle` reorders the tracks first (the album's "Zufall").
	 */
	async function play(itemId, startTrackIndex, { shuffle = false, random } = {}) {
		const res = await request(`/play/${itemId}`, { method: 'POST' });
		if (!res.ok) return false;
		const body = await res.json();
		const tracks = body.tracks;
		if (!tracks?.length) return false;
		if (shuffle) shuffleInPlace(tracks, random);

		let trackIndex;
		let offset;
		let position;
		if (startTrackIndex === undefined && !shuffle) {
			({ trackIndex, offset } = locate(tracks, body.start_position));
			position = body.start_position;
		} else {
			trackIndex = clampIndex(tracks, startTrackIndex ?? 0);
			offset = 0;
			position = trackStart(tracks, trackIndex);
		}

		// Switching items ends the previous session, so its listening time is counted.
		if (current && current.sessionId !== body.session_id) closeSession(current);

		current = {
			itemId,
			kind: body.kind ?? 'album',
			hasCover: Boolean(body.has_cover),
			sessionId: body.session_id,
			tracks,
			chapters: body.chapters ?? [],
			trackIndex,
			position,
			playing: true,
			speed: preferences.playbackSpeed
		};
		startHeartbeat();
		loadTrack(trackIndex, offset);
		notify();
		return true;
	}

	function jumpToTrack(trackIndex) {
		if (!current) return;
		const index = clampIndex(current.tracks, trackIndex);
		current.playing = true;
		current.position = trackStart(current.tracks, index);
		loadTrack(index, 0);
		notify();
	}

	async function playTrackAt(itemId, trackIndex) {
		if (!current || current.itemId !== itemId) {
			await play(itemId, trackIndex);
			return;
		}
		jumpToTrack(trackIndex);
	}

	async function playTrackById(itemId, trackId) {
		if (!current || current.itemId !== itemId) await play(itemId);
		const index = current?.tracks.findIndex((t) => t.id === trackId) ?? -1;
		if (index < 0) return;
		jumpToTrack(index);
	}

	async function playFrom(itemId, position) {
		if (!current || current.itemId !== itemId) await play(itemId);
		if (!current) return;
		current.playing = true;
		seek(position);
		startPlayback();
	}

	function pause() {
		if (!current) return;
		current.playing = false;
		audioEl?.pause();
		notify();
	}

	function resume() {
		if (!current) return;
		current.playing = true;
		if (audioEl && !audioEl.getAttribute('src')) {
			// Nothing loaded yet: pick the track back up.
			const { trackIndex, offset } = locate(current.tracks, current.position);
			loadTrack(trackIndex, offset);
			notify();
			return;
		}
		startPlayback();
		notify();
	}

	function toggle() {
		if (!current) return;
		if (current.playing) pause();
		else resume();
	}

	function seek(seconds) {
		if (!current) return;
		const clamped = Math.max(0, seconds);
		const { trackIndex, offset } = locate(current.tracks, clamped);
		if (trackIndex !== current.trackIndex) {
			loadTrack(trackIndex, offset);
		} else if (audioEl) {
			setOffset(offset);
		}
		current.position = clamped;
		notify();
	}

	function skipBack(seconds = preferences.skipBack) {
		if (current) seek(current.position - seconds);
	}

	function skipForward(seconds = preferences.skipForward) {
		if (current) seek(current.position + seconds);
	}

	function setSpeed(value) {
		const clamped = Math.min(4, Math.max(0.5, value));
		preferences = { ...preferences, playbackSpeed: clamped };
		if (current) current.speed = clamped;
		if (audioEl) audioEl.playbackRate = clamped;
		notify();
	}

	function applyPreferences(next) {
		preferences = { ...preferences, ...next };
		if (current) current.speed = preferences.playbackSpeed;
		if (audioEl) audioEl.playbackRate = preferences.playbackSpeed;
		notify();
	}

	function seekInTrack(offset) {
		if (!current) return;
		seek(trackStart(current.tracks, current.trackIndex) + offset);
	}

	function trackOffset() {
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
	function nudgeStep(direction, kind = current?.kind) {
		if (kind === 'album') return 10;
		return direction < 0 ? preferences.skipBack : preferences.skipForward;
	}

	function nudge(direction) {
		if (!current) return;
		const step = nudgeStep(direction) * direction;
		if (current.kind === 'album') seekInTrack(Math.max(0, trackOffset() + step));
		else seek(current.position + step);
	}

	/**
	 * One step back: the track for an album, the chapter otherwise — and, past three seconds
	 * in, the start of the current one first, like every other player.
	 */
	function jumpPrevious() {
		if (!current) return;
		if (current.kind === 'album' || current.chapters.length === 0) {
			previousTrack();
			return;
		}
		const index = chapterIndexAt(current.chapters, current.position);
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
		const next = current.chapters[chapterIndexAt(current.chapters, current.position) + 1];
		if (next) seek(next.start);
	}

	function reloadCurrentTrack() {
		if (!current) return;
		loadTrack(current.trackIndex, 0);
	}

	/** Saves the position and ends the session (sleep timer, logout). */
	async function close() {
		if (!current) return;
		await flushProgress(false);
		closeSession(current);
		stopHeartbeat();
		if (audioEl) {
			audioEl.pause();
			audioEl.removeAttribute('src');
		}
		current = null;
		notify();
	}

	return {
		get current() {
			return current;
		},
		get preferences() {
			return preferences;
		},
		subscribe,
		setSpeed,
		applyPreferences,
		play,
		playTrackAt,
		playTrackById,
		playFrom,
		pause,
		resume,
		toggle,
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
		flushProgress,
		getAnalyser,
		attachAudioElement,
		reloadCurrentTrack
	};
}
