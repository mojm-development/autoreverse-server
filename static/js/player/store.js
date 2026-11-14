// The web player's state and transport (legacy src/lib/player.svelte.ts), without a framework:
// the store mutates `current` and tells its subscribers. One store per page load; it owns the
// one <audio> element, which lives outside the swapped part of the page and so keeps playing
// across boosted navigations.

import { volumeFor } from './loudness.js';

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
 * The most media time two `timeupdate` events in a row may lie apart and still count as
 * listening. Browsers fire it at least every 250 ms, a throttled background tab about once a
 * second — at the top speed of 4× that is four seconds of media. Anything beyond is a jump the
 * `seeking` event did not announce, and counting it would book time nobody heard.
 */
const MAX_HEARD_STEP = 5;

/**
 * @param {object} [options]
 * @param {Partial<typeof DEFAULT_PREFERENCES>} [options.preferences]
 * @param {typeof fetch} [options.fetch]
 * @param {() => AudioContext | null} [options.createAudioContext]
 * @param {(ended: { itemId: number, trackId: number, last: boolean }) => boolean} [options.onEnded]
 *   Called whenever a track runs out; returning true means the caller plays on itself (a
 *   playlist), so the store does not continue with the item's next track.
 * @param {() => boolean} [options.normalize] Whether the volume is levelled ("Lautstärke
 *   angleichen"); asked for every track, so the switch takes effect without a reload.
 */
export function createPlayerStore(options = {}) {
	const request = options.fetch ?? ((...args) => fetch(...args));
	const onEnded = options.onEnded;
	const normalize = options.normalize ?? (() => true);
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
	// The listener's own volume, 0–1 — set from another device (devices.js) or the devices
	// panel. The levelling below works within it.
	let masterVolume = 1;
	// Where the element stood at the last `timeupdate`, or null right after a jump or a new
	// source: the next event then only sets the mark instead of counting the distance.
	let heardMark = null;
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

	/**
	 * The position as the server counts it: along the item's own track order. A shuffled play
	 * walks a reordered list, so its `position` would point somewhere else in the album.
	 */
	function savedPosition(state) {
		const track = state.tracks[state.trackIndex];
		if (track?.albumStart === undefined) return state.position;
		return track.albumStart + (state.position - trackStart(state.tracks, state.trackIndex));
	}

	/** Music and live radio play at their own speed; the speed setting is for spoken word. */
	function speedFor(kind) {
		return kind === 'album' || kind === 'station' ? 1 : preferences.playbackSpeed;
	}

	async function flushProgress(finished) {
		// A single track out of a playlist or the radio is not a place in its album: saving it
		// would put the album under "Weiter hören" and move where it was left.
		if (!current || current.single) return;
		try {
			await request(`/progress/${current.itemId}`, {
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ position: savedPosition(current), finished })
			});
		} catch {
			// Offline for a moment: the next heartbeat tries again.
		}
	}

	/**
	 * What closing a session sends: where it stands and how much of it was audible. The server
	 * books `listened`; without it, it could only take the distance from where the session
	 * began, which is wrong after any jump (a later chapter, a shuffled album, a rewind).
	 */
	function closeBody(state) {
		const round = (seconds) => Math.round(seconds * 1000) / 1000;
		const listened = round(state.listened);
		// Which tracks that time went to, so the statistics know the songs and not only the
		// album. Left out while nothing was heard: the body then is what it always was.
		const tracks = [...state.heard]
			.map(([trackId, seconds]) => ({ track_id: trackId, seconds: round(seconds) }))
			.filter((share) => share.seconds > 0);
		const shares = tracks.length ? { tracks } : {};
		// Without a position the server books the time and leaves the item's progress alone.
		if (state.single) return { position: null, listened, ...shares };
		const position = savedPosition(state);
		return { position, duration: position, listened, ...shares };
	}

	/** Ends a session on the server and books its listening time; the player carries on. */
	function closeSession(state) {
		// A radio station plays without a session: there is nothing to close or to book.
		if (!state.sessionId) return;
		void request(`/sessions/${state.sessionId}/close`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(closeBody(state))
		}).catch(() => undefined);
	}

	/**
	 * A fresh `src` has no duration yet, and `currentTime` written before the metadata arrives
	 * is dropped on the floor — remember it and write it again from `loadedmetadata`.
	 */
	function setOffset(offset) {
		if (!audioEl) return;
		heardMark = null;
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
					// Playback another device asked for (a take-over) can now only be started
					// by hand; the mini bar offers "Hier weiterhören" while this is set.
					current.refused = name === 'NotAllowedError';
					notify();
				}
			});
	}

	/**
	 * Sets the element's volume for the current track (loudness.js). Music only: spoken word
	 * and radio stations play as they are. An album played as such takes the album's value, so
	 * the volume does not move between its tracks; a single track out of a playlist, a queue or
	 * the Titel-Radio takes its own.
	 */
	function applyVolume() {
		if (!audioEl) return;
		const track = current?.tracks[current.trackIndex];
		const levelled = current && !current.live && current.kind === 'album' && normalize();
		audioEl.volume = masterVolume * (levelled ? volumeFor(track, !current.single) : 1);
	}

	function setVolume(value) {
		const clamped = Math.min(1, Math.max(0, Number(value)));
		if (Number.isNaN(clamped) || clamped === masterVolume) return;
		masterVolume = clamped;
		applyVolume();
		notify();
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
		if (current.live) {
			// A live stream starts at "now"; there is no offset to set.
			audioEl.src = `/radio/stations/${current.stationId}/stream`;
			heardMark = null;
			pendingOffset = null;
		} else {
			audioEl.src = `/tracks/${track.id}/stream`;
			setOffset(offset);
		}
		audioEl.playbackRate = current.speed;
		// With the new source, before it sounds: the next track never starts at the last one's
		// volume.
		applyVolume();
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
				current.refused = false;
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
			heardMark = null;
			audioEl.currentTime = pendingOffset;
			pendingOffset = null;
		};
		// Every jump, also one this store did not make (the lock screen's scrubber).
		audioEl.onseeking = () => {
			heardMark = null;
		};
		audioEl.ontimeupdate = () => {
			if (!current || !audioEl) return;
			// Listening time is the media that actually ran through the element: the steps
			// between two updates while it plays, never the distance of a jump.
			const at = audioEl.currentTime;
			if (heardMark !== null && current.playing) {
				const step = at - heardMark;
				if (step > 0 && step <= MAX_HEARD_STEP) {
					current.listened += step;
					const trackId = current.tracks[current.trackIndex]?.id;
					if (trackId != null) current.heard.set(trackId, (current.heard.get(trackId) ?? 0) + step);
				}
			}
			heardMark = at;
			current.position = trackStart(current.tracks, current.trackIndex) + at;
			notify();
		};
		// A station that cannot be reached or whose stream breaks off: say that it stopped. Only
		// for live streams — a file that fails keeps the behaviour it always had.
		audioEl.onerror = () => {
			if (current?.live && current.playing) {
				current.playing = false;
				notify();
			}
		};
		audioEl.onended = () => {
			if (!current) return;
			if (current.live) {
				// The sender closed the stream. Nothing follows a station: no next track, no
				// autoplay, no progress to mark as finished.
				current.playing = false;
				notify();
				return;
			}
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
	 * `single` marks a session that is about one track only (a playlist entry, the radio): it
	 * counts as listening but never as progress in the item.
	 */
	async function play(itemId, startTrackIndex, { shuffle = false, random, single = false } = {}) {
		const res = await request(`/play/${itemId}`, { method: 'POST' });
		if (!res.ok) return false;
		const body = await res.json();
		const tracks = body.tracks;
		if (!tracks?.length) return false;
		if (shuffle) {
			tracks.forEach((track, index) => {
				track.albumStart = trackStart(tracks, index);
			});
			shuffleInPlace(tracks, random);
		}

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
			single,
			tracks,
			chapters: body.chapters ?? [],
			trackIndex,
			position,
			// Media seconds heard in this session, see `ontimeupdate`.
			listened: 0,
			// The same seconds by track id, in the order the tracks were first heard.
			heard: new Map(),
			playing: true,
			speed: speedFor(body.kind ?? 'album')
		};
		startHeartbeat();
		loadTrack(trackIndex, offset);
		notify();
		return true;
	}

	/**
	 * Plays a radio station live (`GET /radio/stations/{id}/stream`). Unlike `play` this opens
	 * no session and never saves progress: a live stream has no position to come back to and is
	 * not booked as listening time. Whatever played before is closed the way it was heard.
	 * `station` is `{id, name, hasLogo}`.
	 */
	function playStation(station) {
		if (current) closeSession(current);
		stopHeartbeat();
		current = {
			itemId: null,
			stationId: station.id,
			kind: 'station',
			live: true,
			hasCover: false,
			hasLogo: Boolean(station.hasLogo),
			sessionId: null,
			// Guards every progress write, like a single track out of a playlist.
			single: true,
			// One "track" without a length, so everything that reads the current track finds one.
			tracks: [{ id: null, title: station.name ?? '', duration: 0 }],
			chapters: [],
			trackIndex: 0,
			position: 0,
			listened: 0,
			heard: new Map(),
			playing: true,
			speed: 1,
			// `{title, artist, raw}` from `GET /radio/stations/{id}/now`, see setNowPlaying.
			nowPlaying: null
		};
		loadTrack(0, 0);
		notify();
		return true;
	}

	/** What the station plays right now, as the server read it from the stream. */
	function setNowPlaying(stationId, now) {
		if (!current?.live || current.stationId !== stationId) return;
		const next = now?.raw ? { title: now.title ?? null, artist: now.artist ?? null, raw: now.raw } : null;
		if (JSON.stringify(next) === JSON.stringify(current.nowPlaying)) return;
		current.nowPlaying = next;
		notify();
	}

	function jumpToTrack(trackIndex) {
		if (!current || current.live) return;
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

	/**
	 * Plays one track of an item. A change between "this album" and "this one track of it"
	 * (`single`) opens a new session even for the item that is playing, so each is closed the
	 * way it was heard.
	 */
	async function playTrackById(itemId, trackId, { single = false } = {}) {
		if (!current || current.itemId !== itemId || current.single !== single) {
			await play(itemId, undefined, { single });
		}
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
		if (current.live && audioEl) {
			// A paused element would keep the connection open and fall ever further behind the
			// broadcast. Let go of the stream instead; resuming tunes in again.
			audioEl.removeAttribute('src');
			audioEl.load?.();
			// With the stream gone the server no longer knows the title either.
			current.nowPlaying = null;
		}
		notify();
	}

	function resume() {
		if (!current) return;
		current.playing = true;
		if (current.live) {
			// Always from "now", also after a pause this store did not make (a headset button):
			// what the element still has buffered is the past.
			loadTrack(0, 0);
			notify();
			return;
		}
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
		// Nothing to seek in a live stream; this also covers skipping, nudging and the scrubbers.
		if (!current || current.live) return;
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

	function applySpeed() {
		if (!current) return;
		current.speed = speedFor(current.kind);
		if (audioEl) audioEl.playbackRate = current.speed;
	}

	function setSpeed(value) {
		const clamped = Math.min(4, Math.max(0.5, value));
		preferences = { ...preferences, playbackSpeed: clamped };
		applySpeed();
		notify();
	}

	function applyPreferences(next) {
		preferences = { ...preferences, ...next };
		applySpeed();
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
		if (!current || current.live) return;
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
		if (!current || current.live) return;
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
		applyVolume();
		notify();
	}

	return {
		get current() {
			return current;
		},
		get preferences() {
			return preferences;
		},
		get volume() {
			return masterVolume;
		},
		setVolume,
		subscribe,
		setSpeed,
		applyPreferences,
		play,
		playTrackAt,
		playTrackById,
		playFrom,
		playStation,
		setNowPlaying,
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
		savedPosition: () => (current ? savedPosition(current) : 0),
		closeBody: () => (current ? closeBody(current) : null),
		getAnalyser,
		applyVolume,
		attachAudioElement,
		reloadCurrentTrack
	};
}
