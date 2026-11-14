// The playlist queue of the web player: which entry plays, and what "previous" / "next" mean
// while one does. Plain functions over the store's `current`, so tests/js can drive them.
//
// An entry is `{entryId, itemId, trackId, kind}`; `trackId` is null for a whole item. Inside a
// whole album, previous/next step through its tracks first, inside a book or episode through
// its chapters — only at the edges do they move to the neighbouring entry. A single-track entry
// has no inside: next is the next entry, previous restarts it (past three seconds) or goes back.

import { chapterIndexAt } from './store.js';

/** Restart instead of going back once this far into a track or item, like every player. */
const RESTART_AFTER = 3;

export function createQueue(playlistId, entries, index = 0) {
	return { playlistId: String(playlistId), entries, index };
}

/** Whether the store plays `entry` right now. */
export function isEntryPlaying(entry, current) {
	if (!entry || !current || entry.itemId !== current.itemId) return false;
	if (!entry.trackId) return true;
	return current.tracks[current.trackIndex]?.id === entry.trackId;
}

/** Whether the store's own previous/next stays inside the playing whole-item entry. */
function stepsInside(entry, current, direction, trackOffset) {
	if (entry.trackId) return false;
	const byTrack = current.kind === 'album' || current.chapters.length === 0;
	if (direction > 0) {
		if (byTrack) return current.trackIndex < current.tracks.length - 1;
		return chapterIndexAt(current.chapters, current.position) + 1 < current.chapters.length;
	}
	if (byTrack) return current.trackIndex > 0 || trackOffset > RESTART_AFTER;
	return current.position > RESTART_AFTER;
}

/**
 * What previous (`direction` -1) or next (+1) does:
 * - `{ kind: 'store' }` — the store's own jump (inside the entry, or no queue at all),
 * - `{ kind: 'restart' }` — back to the start of the playing single-track entry,
 * - `{ kind: 'entry', index }` — play that entry of the queue,
 * - `{ kind: 'none' }` — nothing after the last entry.
 */
export function queueStep(queue, current, direction, trackOffset) {
	const entry = queue?.entries[queue.index];
	if (!current || !isEntryPlaying(entry, current)) return { kind: 'store' };
	if (stepsInside(entry, current, direction, trackOffset)) return { kind: 'store' };
	if (direction < 0 && entry.trackId && trackOffset > RESTART_AFTER) return { kind: 'restart' };
	const index = queue.index + direction;
	if (index < 0) return entry.trackId ? { kind: 'restart' } : { kind: 'store' };
	if (index >= queue.entries.length) return { kind: 'none' };
	return { kind: 'entry', index };
}

/** The `data-entry-id` of the row to mark on playlist `playlistId`'s page, or null. */
export function playingEntryId(queue, playlistId, current) {
	if (!queue || queue.playlistId !== String(playlistId)) return null;
	const entry = queue.entries[queue.index];
	return isEntryPlaying(entry, current) ? entry.entryId : null;
}

/** The id of the queue the radio plays, where a playlist's queue carries the playlist's. */
export const RADIO_QUEUE = 'radio';
/** What `GET /radio` takes as `exclude` at most. */
const MAX_EXCLUDED = 500;

/** Queue entries for the tracks `GET /radio` answered with: each one a single-track entry. */
export function radioEntries(tracks) {
	return tracks.map((track) => ({
		entryId: `radio-${track.id}`,
		itemId: track.item_id,
		trackId: track.id,
		kind: track.item_kind
	}));
}

/**
 * The query of `GET /radio` for a seed — `{trackId}`, `{itemId}` or `{artist}` — leaving out
 * the tracks already heard, the newest of them when there are too many to name.
 */
export function radioQuery(seed, exclude = []) {
	const query = new URLSearchParams();
	if (seed.trackId) query.set('track_id', String(seed.trackId));
	else if (seed.itemId) query.set('item_id', String(seed.itemId));
	else query.set('artist', seed.artist);
	for (const id of exclude.slice(-MAX_EXCLUDED)) query.append('exclude', String(id));
	return query.toString();
}

const AUTOPLAY_KEY = 'autoreverse-autoplay';

/**
 * Whether music plays on with a radio when the list runs out. Kept in this browser, like the
 * theme: on unless switched off, and on where there is no storage to ask.
 */
export function autoplayEnabled(storage) {
	try {
		return storage.getItem(AUTOPLAY_KEY) !== 'off';
	} catch {
		return true;
	}
}

export function setAutoplay(storage, on) {
	try {
		storage.setItem(AUTOPLAY_KEY, on ? 'on' : 'off');
	} catch {
		// No storage: the switch cannot be kept, autoplay stays on.
	}
}
