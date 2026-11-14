// Remote control between the devices of an account, the browser's side of it (the contract is
// in the README, "Remote control"): this file is the logic without a browser — what this player
// reports as its state, where a watched device stands right now, how a handed-over state is
// played, and which of several open tabs holds the one event stream. devicelink.js does the
// talking, main.js the playing; tests/js drives the functions here.

import { totalDuration, trackStart } from './store.js';
import { RADIO_QUEUE, isEntryPlaying } from './queue.js';

const DEVICE_ID_KEY = 'autoreverse-device-id';
/** The longest queue the server takes. */
export const MAX_QUEUE = 300;
/** While it plays, a device reports at least this often. */
export const REPORT_MS = 10_000;
/** How far a position may be off what the watchers work out before they are told (as the server). */
const JUMP_SECONDS = 3;

function randomId() {
	// `crypto.randomUUID` only exists on https and localhost; a server in the LAN is neither.
	if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
	const bytes = new Uint8Array(16);
	if (globalThis.crypto?.getRandomValues) globalThis.crypto.getRandomValues(bytes);
	else for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
	return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

/**
 * This browser's device id: made once, kept in `localStorage`, so every tab and every visit is
 * the same device. Where there is no storage it lasts for this page.
 */
export function deviceId(storage, create = randomId) {
	try {
		const kept = storage.getItem(DEVICE_ID_KEY);
		if (kept && /^[A-Za-z0-9-]{8,64}$/.test(kept)) return kept;
		const id = create();
		storage.setItem(DEVICE_ID_KEY, id);
		return id;
	} catch {
		return create();
	}
}

/** What the other devices call this one: "Firefox auf Linux". */
export function deviceName(userAgent = '') {
	const first = (table) => table.find(([pattern]) => pattern.test(userAgent))?.[1];
	const browser =
		first([
			[/Edg\//, 'Edge'],
			[/OPR\//, 'Opera'],
			[/Firefox\//, 'Firefox'],
			[/Chrome\//, 'Chrome'],
			[/Safari\//, 'Safari']
		]) ?? 'Browser';
	const platform = first([
		[/iPhone|iPod/, 'iPhone'],
		[/iPad/, 'iPad'],
		[/Android/, 'Android'],
		[/CrOS/, 'ChromeOS'],
		[/Windows/, 'Windows'],
		[/Macintosh|Mac OS X/, 'macOS'],
		[/Linux/, 'Linux']
	]);
	return platform ? `${browser} auf ${platform}` : browser;
}

const KIND_LABELS = { desktop: 'Rechner', phone: 'Telefon', tablet: 'Tablet', web: 'Browser', car: 'Auto' };

export function kindLabel(kind) {
	return KIND_LABELS[kind] ?? 'Gerät';
}

/** "Titel · Interpret", as far as the device said. */
export function mediaLine(media) {
	if (!media) return '';
	return [media.title, media.artist].filter(Boolean).join(' · ');
}

/**
 * What this player reports (`PUT /me/devices/{id}/state`). `current` is the store's, `queue`
 * the playlist or radio being played (queue.js), `item` the `/items/{id}` answer once it is
 * there. Music reports the track and the position within it, a book or an episode the item and
 * the position within the whole. The web player has neither shuffle nor repeat to report.
 */
export function localState({ current, queue = null, item = null, volume = 1, now = Date.now() }) {
	const state = {
		playing: false,
		media: null,
		position: 0,
		duration: 0,
		volume,
		shuffle: false,
		repeat: 'off',
		queue: [],
		context: null,
		at: new Date(now).toISOString()
	};
	if (!current) return state;
	const track = current.tracks[current.trackIndex];
	state.playing = Boolean(current.playing);
	if (current.live) {
		state.media = {
			kind: 'station',
			item_id: null,
			track_id: null,
			station_id: current.stationId,
			title: track?.title ?? '',
			artist: null,
			// The running title, as far as the sender tells.
			item_title: current.nowPlaying?.raw ?? null
		};
		state.context = 'Radio';
		return state;
	}
	const entry = queue?.entries[queue.index];
	const queued = isEntryPlaying(entry, current);
	const following = queued
		? queue.entries
				.slice(queue.index + 1)
				.map((next) => ({ item_id: next.itemId, track_id: next.trackId ?? null }))
		: [];
	if (current.kind === 'album') {
		state.media = {
			kind: 'track',
			item_id: current.itemId,
			track_id: track?.id ?? null,
			station_id: null,
			title: track?.title ?? item?.title ?? '',
			artist: track?.artist ?? item?.artist ?? null,
			item_title: item?.title ?? null
		};
		state.position = Math.max(0, current.position - trackStart(current.tracks, current.trackIndex));
		state.duration = track?.duration ?? 0;
		// One track out of a playlist or the Titel-Radio is followed by the queue's entries; an
		// album played as such by its remaining tracks first.
		const own =
			queued && entry.trackId
				? [{ item_id: current.itemId, track_id: entry.trackId }]
				: current.tracks
						.slice(current.trackIndex)
						.map((next) => ({ item_id: current.itemId, track_id: next.id }));
		state.queue = [...own, ...following].slice(0, MAX_QUEUE);
		state.context =
			queued && queue.playlistId === RADIO_QUEUE ? 'Titel-Radio' : (item?.title ?? null);
		return state;
	}
	state.media = {
		kind: current.kind === 'book' ? 'book' : 'episode',
		item_id: current.itemId,
		track_id: null,
		station_id: null,
		title: item?.title ?? track?.title ?? '',
		artist: item?.author ?? item?.artist ?? null,
		item_title: null
	};
	state.position = Math.max(0, current.position);
	state.duration = totalDuration(current.tracks);
	state.queue = [{ item_id: current.itemId, track_id: null }, ...following].slice(0, MAX_QUEUE);
	state.context = item?.title ?? null;
	return state;
}

/** Whether `next` says more than `sent` plus the time that passed (the server's rule). */
export function differsNotably(sent, next) {
	const frame = (state) => JSON.stringify({ ...state, position: 0, at: '' });
	if (frame(sent) !== frame(next)) return true;
	const elapsed = sent.playing ? (Date.parse(next.at) - Date.parse(sent.at)) / 1000 : 0;
	return Math.abs(next.position - (sent.position + elapsed)) > JUMP_SECONDS;
}

/**
 * Whether to report `next` now: at once when something changed, else every ten seconds while
 * it plays. `sent` is `{state, at}` of the last report, or null.
 */
export function shouldReport(sent, next, now = Date.now()) {
	if (!sent) return true;
	if (differsNotably(sent.state, next)) return true;
	return next.playing && now - sent.at >= REPORT_MS;
}

/**
 * Where a watched device stands now: its reported position plus the time since, while it
 * plays. Never past the end, never before the report (a clock that runs behind), and a live
 * station has no position at all.
 */
export function projectedPosition(state, now = Date.now()) {
	if (!state) return 0;
	const position = Number(state.position) || 0;
	if (!state.playing || state.media?.kind === 'station') return position;
	const elapsed = Math.max(0, (now - Date.parse(state.at)) / 1000);
	const projected = position + (Number.isFinite(elapsed) ? elapsed : 0);
	return state.duration > 0 ? Math.min(projected, state.duration) : projected;
}

/**
 * The other device whose playback the mini bar shows while nothing plays here: the active one,
 * else one that plays. Null when the others are silent.
 */
export function watchedDevice(devices, selfId) {
	const others = (devices ?? []).filter(
		(device) => device.id !== selfId && device.state?.media && (device.active || device.state.playing)
	);
	return others.find((device) => device.active) ?? others[0] ?? null;
}

/** The id of the queue a take-over plays, beside a playlist's id and the radio's. */
export const REMOTE_QUEUE = 'remote';

/**
 * How to play a state handed over by another device (`take_over`):
 * - `{kind: 'station', stationId, title}`,
 * - `{kind: 'item', itemId, itemKind, position}` — a book or an episode, position in the whole,
 * - `{kind: 'tracks', entries, trackId, position}` — music: queue entries (queue.js) from the
 *   running one on, the running track's id where the first entry is a whole album, and the
 *   position within that track,
 * - null when there is nothing to play.
 */
export function takeOverPlan(state) {
	const media = state?.media;
	if (!media) return null;
	const position = Math.max(0, Number(state.position) || 0);
	if (media.kind === 'station') {
		return media.station_id ? { kind: 'station', stationId: media.station_id, title: media.title ?? '' } : null;
	}
	if (media.kind === 'book' || media.kind === 'episode') {
		return media.item_id ? { kind: 'item', itemId: media.item_id, itemKind: media.kind, position } : null;
	}
	let listed = (state.queue ?? []).filter((entry) => entry?.item_id);
	// A device that reports no queue still plays something: the medium itself.
	if (!listed.length && media.item_id) listed = [{ item_id: media.item_id, track_id: media.track_id ?? null }];
	if (!listed.length) return null;
	const entries = listed.map((entry, index) => ({
		entryId: `remote-${index}`,
		itemId: entry.item_id,
		trackId: entry.track_id ?? null,
		kind: 'album'
	}));
	return { kind: 'tracks', entries, trackId: media.track_id ?? null, position };
}

/** A tab that holds the stream says so this often; see createElection. */
export const BEACON_MS = 5_000;
const STALE_MS = 3 * BEACON_MS;
const ANSWER_MS = 250;

/**
 * Which of the open tabs holds the event stream. All tabs of a browser are one device (one id),
 * and the server ends a device's stream when a second one opens — two tabs that both insist
 * would throw each other out forever. So exactly one holds it, and the tabs agree over a
 * `BroadcastChannel`:
 *
 * - a new tab asks ("who"); without an answer it claims the stream,
 * - a tab that starts to play claims it (`wantHold`): commands must reach the tab that plays,
 * - of two claims the playing tab wins, then the later claim, then the larger tab id — the
 *   loser lets go (`onChange(false)`) and never reconnects by itself,
 * - the holder repeats its claim every few seconds (`tick`), so a tab that was frozen and wakes
 *   up believing it still holds learns otherwise, and so a holder that vanished without a word
 *   is replaced by a visible tab,
 * - a holder that leaves says "bye"; one of the others claims after a random moment.
 *
 * `post` sends a message to the other tabs (a no-op where there is no channel: the tab then
 * simply holds), `receive` takes theirs.
 */
export function createElection({
	tabId,
	post,
	onChange,
	now = Date.now,
	later = setTimeout,
	random = Math.random,
	isPlaying = () => false,
	isVisible = () => true
}) {
	let holding = false;
	let since = 0;
	/** The other tab that holds, as far as known. */
	let holder = null;
	let heardAt = 0;

	const mine = () => ({ t: 'hold', tab: tabId, since, playing: Boolean(isPlaying()) });
	function beats(a, b) {
		if (Boolean(a.playing) !== Boolean(b.playing)) return Boolean(a.playing);
		if (a.since !== b.since) return a.since > b.since;
		return a.tab > b.tab;
	}

	function claim() {
		const held = holding;
		since = now();
		holder = null;
		// Holding before it is said: the answer of a tab with the better claim must find a
		// holder to unseat, whenever it arrives.
		holding = true;
		post(mine());
		if (!held && holding) onChange(true);
	}

	function claimIfFree() {
		if (!holding && !holder) claim();
	}

	function receive(message) {
		if (!message || message.tab === tabId) return;
		if (message.t === 'who') {
			if (holding) post(mine());
		} else if (message.t === 'hold') {
			if (!holding) {
				holder = message;
				heardAt = now();
			} else if (beats(message, mine())) {
				holding = false;
				holder = message;
				heardAt = now();
				onChange(false);
			} else {
				// The other one has to hear that it lost.
				post(mine());
			}
		} else if (message.t === 'bye' && holder?.tab === message.tab) {
			holder = null;
			later(claimIfFree, Math.floor(random() * ANSWER_MS));
		}
	}

	return {
		get holding() {
			return holding;
		},
		receive,
		start() {
			post({ t: 'who', tab: tabId });
			later(claimIfFree, ANSWER_MS);
		},
		/** This tab plays (or is asked to take over): the stream belongs here. */
		wantHold() {
			if (!holding) claim();
		},
		/** Every `BEACON_MS`, and when the tab becomes visible. */
		tick() {
			if (holding) {
				post(mine());
				return;
			}
			if (holder && now() - heardAt <= STALE_MS) return;
			// A hidden tab's timers run late; it would mistake its own sleep for the holder's.
			if (!isVisible()) return;
			holder = null;
			claim();
		},
		leave() {
			if (!holding) return;
			holding = false;
			post({ t: 'bye', tab: tabId });
		}
	};
}
