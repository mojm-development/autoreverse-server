// The web player: one store for the whole visit, bound to the `<audio>` element of the player
// layout. Page actions are `[data-action]` buttons, handled here by delegation.

import { createPlayerStore } from './store.js';
import { bindPlayerShortcuts } from './shortcuts.js';
import { bindScrubbers } from './scrubber.js';
import { createVisualizers } from './visualizer.js';
import { installNavigation } from './navigation.js';
import { render, renderPlaylistRows } from './ui.js';
import { formatHMS } from './format.js';
import { createQueue, isEntryPlaying, playingEntryId, queueStep } from './queue.js';
import { bindMediaSession } from './mediasession.js';

const audio = document.querySelector('[data-player-audio]');

/** The preferences the server rendered into `#page`. */
function pagePreferences() {
	const page = document.getElementById('page');
	if (!page) return null;
	const speed = Number(page.dataset.speed);
	const back = Number(page.dataset.skipBack);
	const forward = Number(page.dataset.skipForward);
	if (!speed || !back || !forward) return null;
	return { playbackSpeed: speed, skipBack: back, skipForward: forward };
}

if (audio) {
	let lastServerPreferences = pagePreferences();
	/**
	 * The playlist being played (queue.js): its entries and which one plays. Playing anything
	 * else outside it ends it.
	 */
	let queue = null;
	const store = createPlayerStore({
		preferences: lastServerPreferences ?? undefined,
		onEnded: (ended) => onTrackEnded(ended)
	});
	store.attachAudioElement(audio);
	const visualizers = createVisualizers({
		isPlaying: () => Boolean(store.current?.playing),
		getAnalyser: () => store.getAnalyser()
	});

	let scheduled = false;
	function scheduleRender() {
		if (scheduled) return;
		scheduled = true;
		requestAnimationFrame(() => {
			scheduled = false;
			render(store, queueState());
			for (const root of document.querySelectorAll('[data-playlist-id]')) {
				renderPlaylistRows(root, playingEntryId(queue, root.dataset.playlistId, store.current));
			}
			visualizers.sync();
		});
	}
	store.subscribe(scheduleRender);

	const pageItemId = (element) => Number(element.closest('[data-item-id]')?.dataset.itemId);

	// Fullscreen player: the page's item plays unless it already does (legacy onMount).
	const mounted = new WeakSet();
	let sessionTimer = null;
	function mountPage() {
		const preferences = pagePreferences();
		if (
			preferences &&
			JSON.stringify(preferences) !== JSON.stringify(lastServerPreferences)
		) {
			// Settings → Wiedergabe saved new values; the running player takes them over.
			lastServerPreferences = preferences;
			store.applyPreferences(preferences);
		}
		const np = document.querySelector('[data-np]');
		if (np && !mounted.has(np)) {
			mounted.add(np);
			const itemId = Number(np.dataset.itemId);
			if (store.current?.itemId !== itemId) void store.play(itemId);
			const started = Date.now();
			const session = np.querySelector('[data-np-session]');
			if (sessionTimer) clearInterval(sessionTimer);
			sessionTimer = setInterval(() => {
				if (!session?.isConnected) {
					clearInterval(sessionTimer);
					sessionTimer = null;
					return;
				}
				session.textContent = `Sitzung offen · ${Math.round((Date.now() - started) / 60_000)} Min`;
			}, 30_000);
		}
		scheduleRender();
	}

	installNavigation({ onPageChange: mountPage });

	bindScrubbers((kind, seconds) => {
		const current = store.current;
		if (!current) return;
		if (kind === 'full') {
			const np = document.querySelector('[data-np]');
			if (np && current.itemId !== Number(np.dataset.itemId)) return;
		}
		if (current.kind === 'album') store.seekInTrack(seconds);
		else store.seek(seconds);
	});

	/** Previous/next while a playlist plays: `{previous, next}` enabled, or null without one. */
	function queueState() {
		const current = store.current;
		if (!queue || !current || !isEntryPlaying(queue.entries[queue.index], current)) return null;
		return { previous: true, next: queueStep(queue, current, 1, store.trackOffset()).kind !== 'none' };
	}

	/** Starts one entry; true once it plays. */
	async function playEntry(entry) {
		if (entry.trackId) {
			await store.playTrackById(entry.itemId, entry.trackId);
			return isEntryPlaying(entry, store.current);
		}
		// A whole album plays from its first track; a book or episode where it was left.
		if (entry.kind === 'album') return store.play(entry.itemId, 0);
		if (await store.play(entry.itemId)) return true;
		// An episode not downloaded yet is fetched first, as its own play button does.
		return entry.kind === 'episode' && (await downloadEpisode(entry.itemId)) && store.play(entry.itemId);
	}

	/**
	 * The fullscreen player follows the playlist to its next item, without a history entry:
	 * "back" still leads to where the listener came from.
	 */
	function followFullscreen(itemId) {
		const np = document.querySelector('[data-np]');
		if (!np || Number(np.dataset.itemId) === itemId) return;
		const href = `/library/${itemId}/player`;
		if (!globalThis.htmx) {
			location.replace(href);
			return;
		}
		void globalThis.htmx
			.ajax('GET', href, { target: '#page', select: '#page', swap: 'outerHTML' })
			.then(() => history.replaceState(history.state, '', href));
	}

	/**
	 * Plays entry `index` of the queue; past the end the queue is over. An entry that cannot play
	 * (its files are gone) is skipped, so one broken entry does not stop the playlist.
	 */
	async function playQueueAt(index) {
		const mine = queue;
		for (let i = index; mine && i < mine.entries.length; i++) {
			mine.index = i;
			const entry = mine.entries[i];
			if (await playEntry(entry)) {
				if (queue === mine) followFullscreen(entry.itemId);
				scheduleRender();
				return;
			}
			if (queue !== mine) return;
		}
		if (queue === mine) queue = null;
	}

	/** The rows of the playlist page around `element`, in their current order. */
	function startPlaylist(element, index) {
		const page = element.closest('[data-playlist-id]');
		if (!page) return;
		const entries = [...page.querySelectorAll('[data-entry-id]')].map((row) => ({
			entryId: row.dataset.entryId,
			itemId: Number(row.dataset.entryItem),
			trackId: row.dataset.entryTrack ? Number(row.dataset.entryTrack) : null,
			kind: row.dataset.entryKind
		}));
		if (!entries.length) return;
		queue = createQueue(page.dataset.playlistId, entries);
		void playQueueAt(index);
	}

	/**
	 * Previous (-1) or next (+1) in the mini bar, the fullscreen player, the keyboard and the
	 * lock screen: inside a playlist across its entries (queue.js), else tracks or chapters.
	 */
	function step(direction) {
		const current = store.current;
		if (!current) return;
		const result = queue
			? queueStep(queue, current, direction, store.trackOffset())
			: { kind: 'store' };
		if (result.kind === 'entry') void playQueueAt(result.index);
		else if (result.kind === 'restart') store.seekInTrack(0);
		else if (result.kind === 'store') {
			if (direction < 0) store.jumpPrevious();
			else store.jumpNext();
		}
	}

	/**
	 * A track ran out. Inside a playlist: a single-track entry is done right away, a whole item
	 * once its last track is; then the next entry plays. True when the queue took over.
	 */
	function onTrackEnded({ itemId, trackId, last }) {
		const entry = queue?.entries[queue.index];
		if (!entry || entry.itemId !== itemId || (entry.trackId && entry.trackId !== trackId)) {
			queue = null;
			return false;
		}
		if (!entry.trackId && !last) return false;
		if (queue.index + 1 < queue.entries.length) {
			void playQueueAt(queue.index + 1);
			return true;
		}
		queue = null;
		// The playlist is over: a single track must not run on into the rest of its album.
		if (!last) store.pause();
		return true;
	}

	async function restart(itemId) {
		if (await store.play(itemId)) store.seek(0);
	}

	/**
	 * Re-reads the current page and swaps in the parts marked `[data-refresh]` (bookmark list and
	 * count), leaving everything else — open tab, scroll position, the player — as it is.
	 */
	async function refreshParts() {
		const res = await fetch(location.href, { headers: { accept: 'text/html' } });
		if (!res.ok) return;
		const fresh = new DOMParser().parseFromString(await res.text(), 'text/html');
		for (const part of document.querySelectorAll('[data-refresh]')) {
			const next = fresh.querySelector(`[data-refresh="${part.dataset.refresh}"]`);
			if (next) part.innerHTML = next.innerHTML;
		}
	}

	/** "Lesezeichen setzen": where the player is in this item, else at the saved position. */
	async function addBookmark(button) {
		const itemId = pageItemId(button);
		if (!itemId || button.dataset.busy) return;
		const current = store.current;
		const position =
			current?.itemId === itemId ? current.position : Number(button.dataset.position) || 0;
		const label = button.querySelector('[data-bookmark-label]');
		button.dataset.busy = '1';
		try {
			const res = await fetch('/bookmarks', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ item_id: itemId, position, title: formatHMS(position) })
			});
			if (!res.ok) return;
			await refreshParts();
			if (label) {
				label.textContent = 'Lesezeichen gesetzt';
				setTimeout(() => (label.textContent = 'Lesezeichen setzen'), 2000);
			}
		} catch {
			// Offline: nothing was set, the button stays as it was.
		} finally {
			delete button.dataset.busy;
		}
	}

	/** Shows an episode as downloaded wherever the page lists it. */
	function markDownloaded(episodeId) {
		for (const card of document.querySelectorAll(`[data-episode="${episodeId}"]`)) {
			card.dataset.downloaded = 'true';
			card.querySelector('[data-downloaded-badge]')?.removeAttribute('hidden');
			card.querySelector('[data-action="download-episode"]')?.remove();
		}
	}

	/** POST /episodes/{id}/download; true once the episode has a file. */
	async function downloadEpisode(episodeId, button) {
		if (!episodeId || button?.getAttribute('aria-busy') === 'true') return false;
		button?.setAttribute('aria-busy', 'true');
		if (button) button.disabled = true;
		try {
			const res = await fetch(`/episodes/${episodeId}/download`, { method: 'POST' });
			if (!res.ok) return false;
			markDownloaded(episodeId);
			return true;
		} catch {
			return false;
		} finally {
			button?.removeAttribute('aria-busy');
			if (button?.isConnected) button.disabled = false;
		}
	}

	/** An episode plays once it is downloaded; one that is not yet is downloaded first. */
	async function playEpisode(button) {
		const episodeId = Number(button.dataset.episodeId);
		if (!episodeId) return;
		const card = document.querySelector(`[data-episode="${episodeId}"]`);
		if (card?.dataset.downloaded !== 'true' && !(await downloadEpisode(episodeId, button))) return;
		void store.play(episodeId);
	}

	const ACTIONS = {
		toggle: () => store.toggle(),
		'skip-back': () => store.skipBack(),
		'skip-forward': () => store.skipForward(),
		previous: () => step(-1),
		next: () => step(1),
		'nudge-back': () => store.nudge(-1),
		'nudge-forward': () => store.nudge(1),
		'jump-previous': () => step(-1),
		'jump-next': () => step(1),
		play: (el) => void store.play(pageItemId(el)),
		restart: (el) => void restart(pageItemId(el)),
		shuffle: (el) => void store.play(pageItemId(el), undefined, { shuffle: true }),
		'play-track': (el) => void store.playTrackById(pageItemId(el), Number(el.dataset.trackId)),
		'play-from': (el) => void store.playFrom(pageItemId(el), Number(el.dataset.position)),
		bookmark: (el) => void addBookmark(el),
		'play-episode': (el) => void playEpisode(el),
		'download-episode': (el) => void downloadEpisode(Number(el.dataset.episodeId), el),
		'play-playlist': (el) => startPlaylist(el, 0),
		'play-entry': (el) => startPlaylist(el, Number(el.dataset.entryIndex) || 0),
		close: () => history.back()
	};

	const QUEUE_ENDING = new Set([
		'play',
		'restart',
		'shuffle',
		'play-track',
		'play-from',
		'play-episode'
	]);

	// Capture phase: a play button inside a tile link must act before htmx follows the link.
	document.addEventListener(
		'click',
		(event) => {
			const element = event.target instanceof Element ? event.target.closest('[data-action]') : null;
			const action = element && ACTIONS[element.dataset.action];
			if (!action || element.disabled) return;
			event.preventDefault();
			event.stopPropagation();
			// Playing something else leaves the playlist; transport buttons do not.
			if (QUEUE_ENDING.has(element.dataset.action)) queue = null;
			action(element);
		},
		true
	);

	bindPlayerShortcuts((action) => {
		const fullscreen = document.querySelector('[data-np]');
		if (!fullscreen && !store.current) return false;
		if (action === 'close') {
			if (!fullscreen) return false;
			history.back();
			return true;
		}
		if (action === 'toggle') store.toggle();
		else if (action === 'back') store.nudge(-1);
		else if (action === 'forward') store.nudge(1);
		else if (action === 'previous') step(-1);
		else if (action === 'next') step(1);
		return true;
	});

	let sleepTimer = null;
	document.addEventListener('change', (event) => {
		const target = event.target;
		if (!(target instanceof HTMLSelectElement)) return;
		if (target.matches('[data-np-speed]')) {
			const value = Number(target.value);
			store.setSpeed(value);
			const { skipBack, skipForward } = store.preferences;
			void fetch('/me/playback', {
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ playback_speed: value, skip_back: skipBack, skip_forward: skipForward })
			}).then(
				(res) => {
					if (res.ok) {
						lastServerPreferences = { playbackSpeed: value, skipBack, skipForward };
					}
				},
				() => undefined
			);
		} else if (target.matches('[data-np-sleep]')) {
			if (sleepTimer) clearTimeout(sleepTimer);
			sleepTimer = null;
			const minutes = Number(target.value);
			if (minutes > 0) sleepTimer = setTimeout(() => store.pause(), minutes * 60_000);
		}
	});

	// Leaving the site: save where we are and end the session (legacy saved the position only).
	window.addEventListener('pagehide', () => {
		const current = store.current;
		if (!current) return;
		const json = (body) => new Blob([JSON.stringify(body)], { type: 'application/json' });
		navigator.sendBeacon(
			`/progress/${current.itemId}`,
			json({ position: current.position, finished: false })
		);
		navigator.sendBeacon(
			`/sessions/${current.sessionId}/close`,
			json({ position: current.position, duration: current.position })
		);
	});

	bindMediaSession(store, {
		previous: () => step(-1),
		next: () => step(1),
		fetchItem: async (itemId) => {
			try {
				const res = await fetch(`/items/${itemId}`);
				return res.ok ? await res.json() : null;
			} catch {
				return null;
			}
		}
	});

	mountPage();
}
