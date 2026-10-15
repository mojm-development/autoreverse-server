// Media Session API: the lock screen, the OS media overlay and hardware keys (headset buttons,
// keyboard media keys) show what plays and drive the player. The browser only shows it while
// the <audio> element plays; everything here degrades to nothing where the API is missing.

import { chapterIndexAt, totalDuration } from './store.js';

/**
 * What the lock screen shows. `item` is the `/items/{id}` answer (or null while it loads):
 * an album shows the track over artist and album, a book or episode the chapter (if any) over
 * author and title.
 */
export function sessionMetadata(current, item) {
	const track = current.tracks[current.trackIndex];
	const itemTitle = item?.title ?? '';
	let title;
	let album = itemTitle;
	if (current.kind === 'album') {
		title = track?.title || itemTitle;
	} else {
		const chapter = current.chapters[chapterIndexAt(current.chapters, current.position)];
		title = chapter?.title || (current.tracks.length > 1 ? track?.title : '') || itemTitle;
		if (title === itemTitle) album = '';
	}
	return {
		title,
		artist: item?.artist || item?.author || '',
		album,
		artwork: current.hasCover ? [{ src: `/items/${current.itemId}/cover?size=512` }] : []
	};
}

/** `setPositionState` arguments: per track for an album (as the mini bar), else the whole item. */
export function positionState(current, trackOffset) {
	const byTrack = current.kind === 'album';
	const duration = byTrack
		? (current.tracks[current.trackIndex]?.duration ?? 0)
		: totalDuration(current.tracks);
	const position = byTrack ? trackOffset : current.position;
	return {
		duration: Math.max(0, duration),
		position: Math.min(Math.max(0, position), Math.max(0, duration)),
		playbackRate: current.speed || 1
	};
}

/**
 * Binds the store to `navigator.mediaSession`. `previous` / `next` are the queue-aware jumps of
 * the player (playlist entries, tracks, chapters).
 */
export function bindMediaSession(store, { previous, next, fetchItem }) {
	const session = globalThis.navigator?.mediaSession;
	if (!session || typeof globalThis.MediaMetadata !== 'function') return;
	const items = new Map();
	let shownKey = '';

	function handle(action, handler) {
		try {
			session.setActionHandler(action, handler);
		} catch {
			// This browser does not know the action.
		}
	}
	handle('play', () => store.resume());
	handle('pause', () => store.pause());
	handle('stop', () => store.pause());
	handle('previoustrack', () => previous());
	handle('nexttrack', () => next());
	handle('seekbackward', (details) => store.skipBack(details?.seekOffset || undefined));
	handle('seekforward', (details) => store.skipForward(details?.seekOffset || undefined));
	handle('seekto', (details) => {
		const current = store.current;
		if (!current || typeof details?.seekTime !== 'number') return;
		if (current.kind === 'album') store.seekInTrack(details.seekTime);
		else store.seek(details.seekTime);
	});

	function sync() {
		const current = store.current;
		if (!current) {
			if (shownKey) {
				session.metadata = null;
				session.playbackState = 'none';
				shownKey = '';
			}
			return;
		}
		session.playbackState = current.playing ? 'playing' : 'paused';
		if (!items.has(current.itemId)) {
			items.set(current.itemId, null);
			void fetchItem(current.itemId).then((item) => {
				items.set(current.itemId, item);
				shownKey = '';
				sync();
			});
		}
		const metadata = sessionMetadata(current, items.get(current.itemId));
		const key = JSON.stringify(metadata);
		if (key !== shownKey) {
			shownKey = key;
			session.metadata = new globalThis.MediaMetadata(metadata);
		}
		try {
			session.setPositionState?.(positionState(current, store.trackOffset()));
		} catch {
			// A duration of 0 (not loaded yet) is refused; the next update sets it.
		}
	}
	store.subscribe(sync);
	sync();
}
