// The web player: one store for the whole visit, bound to the `<audio>` element of the player
// layout. Page actions are `[data-action]` buttons, handled here by delegation.

import { createPlayerStore } from './store.js';
import { bindPlayerShortcuts } from './shortcuts.js';
import { bindScrubbers } from './scrubber.js';
import { createVisualizers } from './visualizer.js';
import { installNavigation } from './navigation.js';
import { render, renderPlaylistRows } from './ui.js';
import { formatHMS } from './format.js';
import {
	RADIO_QUEUE,
	autoplayEnabled,
	createQueue,
	isEntryPlaying,
	playingEntryId,
	queueStep,
	radioEntries,
	radioQuery,
	setAutoplay
} from './queue.js';
import { bindMediaSession } from './mediasession.js';
import { createLyrics } from './lyrics.js';
import { normalizeEnabled, setNormalize } from './loudness.js';
import { REMOTE_QUEUE, projectedPosition, takeOverPlan, watchedDevice } from './devices.js';
import { createDeviceLink } from './devicelink.js';
import { renderDevicesPanel } from './devices-ui.js';

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
	// `localStorage` is only touched inside these calls: where a browser refuses it, reading the
	// property already throws, and the callers in queue.js are the ones that catch.
	const storage = {
		getItem: (key) => localStorage.getItem(key),
		setItem: (key, value) => localStorage.setItem(key, value)
	};
	/** The tracks the running radio has queued so far, so that continuing it brings new ones. */
	let radioHeard = [];
	const store = createPlayerStore({
		preferences: lastServerPreferences ?? undefined,
		onEnded: (ended) => onTrackEnded(ended),
		normalize: () => normalizeEnabled(storage)
	});
	store.attachAudioElement(audio);
	const visualizers = createVisualizers({
		isPlaying: () => Boolean(store.current?.playing),
		getAnalyser: () => store.getAnalyser()
	});

	const lyrics = createLyrics();

	/** `/items/{id}`, asked once per item: the lock screen and the remote control both name it. */
	const itemRequests = new Map();
	const itemAnswers = new Map();
	function fetchItem(itemId) {
		if (!itemRequests.has(itemId)) {
			const request = fetch(`/items/${itemId}`)
				.then((res) => (res.ok ? res.json() : null))
				.catch(() => null);
			itemRequests.set(itemId, request);
			void request.then((item) => {
				itemAnswers.set(itemId, item);
				// The state reported so far had no title for the item.
				void link?.reportNow();
			});
		}
		return itemRequests.get(itemId);
	}

	// The listener's volume is kept in this browser, like the other playback switches.
	const VOLUME_KEY = 'autoreverse-volume';
	try {
		const kept = localStorage.getItem(VOLUME_KEY);
		if (kept !== null && kept !== '') store.setVolume(Number(kept));
	} catch {
		// No storage: full volume.
	}
	let keptVolume = store.volume;
	store.subscribe(() => {
		if (store.volume === keptVolume) return;
		keptVolume = store.volume;
		try {
			localStorage.setItem(VOLUME_KEY, String(keptVolume));
		} catch {
			// Not kept, then.
		}
	});

	// Remote control (devices.js): this browser is a device of the account — unless the server
	// has the feature switched off, which the layout says on the mini bar.
	const devicesPanel = document.querySelector('[data-devices-panel]');
	let devicesError = '';
	const link =
		document.querySelector('[data-mini-player]')?.dataset.devices === 'true' &&
		typeof globalThis.EventSource === 'function'
			? createDeviceLink({
					store,
					getQueue: () => queue,
					itemInfo: (itemId) => {
						void fetchItem(itemId);
						return itemAnswers.get(itemId) ?? null;
					},
					onDevices: () => scheduleRender(),
					onCommand: (command) => runCommand(command),
					onTakeOver: (takeOver) => void takeOverHere(takeOver)
				})
			: null;

	/** Another device's playback, shown in the mini bar while nothing plays here; else null. */
	function remoteView() {
		if (!link || store.current) return null;
		const device = watchedDevice(link.devices, link.selfId);
		return device ? { device, position: projectedPosition(device.state, Date.now()) } : null;
	}

	// The other device sends no ticks; while it plays, its position is drawn once a second.
	let remoteTicker = null;
	function syncRemoteTicker(view) {
		const wanted = Boolean(view?.device.state.playing);
		if (wanted && !remoteTicker) remoteTicker = setInterval(() => scheduleRender(), 1000);
		if (!wanted && remoteTicker) {
			clearInterval(remoteTicker);
			remoteTicker = null;
		}
	}

	let scheduled = false;
	function scheduleRender() {
		if (scheduled) return;
		scheduled = true;
		requestAnimationFrame(() => {
			scheduled = false;
			const remote = remoteView();
			syncRemoteTicker(remote);
			render(store, queueState(), remote);
			if (link) {
				renderDevicesPanel(devicesPanel, {
					devices: link.devices,
					selfId: link.selfId,
					error: devicesError,
					volume: store.volume
				});
			}
			lyrics.render(document.querySelector('[data-np]'), store);
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
		// Settings → Wiedergabe: the switch shows what this browser has stored.
		for (const toggle of document.querySelectorAll('[data-autoplay-toggle]')) {
			toggle.checked = autoplayEnabled(storage);
		}
		for (const toggle of document.querySelectorAll('[data-normalize-toggle]')) {
			toggle.checked = normalizeEnabled(storage);
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
		const remote = remoteView();
		if (remote && kind === 'mini') {
			void link.send(remote.device.id, 'seek', seconds);
			return;
		}
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
			// One track, not the album it is from: heard, but no progress in the album.
			await store.playTrackById(entry.itemId, entry.trackId, { single: true });
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

	/** GET /radio; an empty list when there is nothing to play or no answer. */
	async function fetchRadio(seed, exclude) {
		try {
			const res = await fetch(`/radio?${radioQuery(seed, exclude)}`);
			if (!res.ok) return [];
			return (await res.json()).tracks ?? [];
		} catch {
			return [];
		}
	}

	/**
	 * "Titel-Radio starten": similar tracks from the library as a queue of single tracks. `first` is
	 * the seed track's own entry, which leads the queue — and simply stays on if it is what
	 * plays already. False when the library has nothing to offer.
	 */
	async function startRadio(seed, first = null, exclude = []) {
		const tracks = await fetchRadio(seed, exclude);
		const entries = [...(first ? [first] : []), ...radioEntries(tracks)];
		if (!entries.length) return false;
		radioHeard = [...exclude, ...entries.map((entry) => entry.trackId)];
		queue = createQueue(RADIO_QUEUE, entries);
		if (first && isEntryPlaying(first, store.current)) scheduleRender();
		else await playQueueAt(0);
		return true;
	}

	const radioEntry = (itemId, trackId) => ({
		entryId: `radio-${trackId}`,
		itemId,
		trackId,
		kind: 'album'
	});

	/** The "Titel-Radio starten" buttons: of a track row, of an album page, of the playing track. */
	function radioFrom(element) {
		const trackId = Number(element.dataset.trackId);
		const itemId = pageItemId(element);
		if (trackId) void startRadio({ trackId }, radioEntry(itemId, trackId));
		else if (itemId) void startRadio({ itemId });
	}

	function radioFromPlaying() {
		const current = store.current;
		const track = current?.tracks[current.trackIndex];
		if (!track || current.kind !== 'album') return;
		void startRadio({ trackId: track.id }, radioEntry(current.itemId, track.id));
	}

	/**
	 * Autoplay: the list is over (an album's last track, a playlist's or a radio's last entry),
	 * so music plays on with a radio from that last track. True when it takes over — only for
	 * music, and only while the switch under Einstellungen → Wiedergabe is on.
	 */
	function continueWithRadio(trackId, last, heard) {
		const ended = store.current;
		if (!ended || ended.kind !== 'album' || !trackId || !autoplayEnabled(storage)) return false;
		void (async () => {
			const tracks = await fetchRadio({ trackId }, heard);
			// Whatever the listener started meanwhile wins.
			if (store.current !== ended || queue) return;
			if (!tracks.length) {
				// Nothing to go on with: a single track must not run on into its album.
				if (!last) store.pause();
				return;
			}
			radioHeard = [...heard, ...tracks.map((track) => track.id)].slice(-500);
			queue = createQueue(RADIO_QUEUE, radioEntries(tracks));
			await playQueueAt(0);
		})();
		return true;
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
			// An album played to its end: on with a radio that leaves the album out.
			if (!last) return false;
			const album = store.current?.tracks.map((track) => track.id) ?? [];
			return continueWithRadio(trackId, last, album);
		}
		if (!entry.trackId && !last) return false;
		if (queue.index + 1 < queue.entries.length) {
			void playQueueAt(queue.index + 1);
			return true;
		}
		const wasRadio = queue.playlistId === RADIO_QUEUE;
		const heard = queue.entries.map((queued) => queued.trackId).filter(Boolean);
		queue = null;
		if (continueWithRadio(trackId, last, wasRadio ? radioHeard : heard)) return true;
		// The playlist is over: a single track must not run on into the rest of its album.
		if (!last) store.pause();
		return true;
	}

	/**
	 * A station tile of the radio page: tunes in, or — on the station that plays — pauses and
	 * resumes. A station is no part of a queue and nothing follows it: autoplay applies to
	 * music that ends, and a live stream does not.
	 */
	function playStation(element) {
		const tile = element.closest('[data-station-id]');
		const id = Number(tile?.dataset.stationId);
		if (!id) return;
		if (store.current?.live && store.current.stationId === id) {
			store.toggle();
			return;
		}
		queue = null;
		store.playStation({
			id,
			name: tile.dataset.stationName ?? '',
			hasLogo: tile.dataset.stationLogo === 'true'
		});
	}

	// The running title of a station: asked for while it plays, and only then — the server
	// knows it from the stream it passes on to this very player.
	const NOW_PLAYING_MS = 12_000;
	let nowTimer = null;
	let nowStation = null;
	async function fetchNowPlaying(stationId) {
		try {
			const res = await fetch(`/radio/stations/${stationId}/now`);
			if (res.ok) store.setNowPlaying(stationId, await res.json());
		} catch {
			// Offline for a moment: the next round asks again.
		}
	}
	function watchNowPlaying() {
		const current = store.current;
		const wanted = current?.live && current.playing ? current.stationId : null;
		if (wanted === nowStation) return;
		nowStation = wanted;
		if (nowTimer) clearInterval(nowTimer);
		nowTimer = null;
		if (wanted === null) return;
		// The first title arrives with the first metadata block, a moment after the stream.
		setTimeout(() => {
			if (nowStation === wanted) void fetchNowPlaying(wanted);
		}, 2500);
		nowTimer = setInterval(() => void fetchNowPlaying(wanted), NOW_PLAYING_MS);
	}
	store.subscribe(watchNowPlaying);

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

	/**
	 * A command another device sent to this one. The web player has neither shuffle nor repeat,
	 * so those two are taken and ignored.
	 */
	function runCommand({ command, value }) {
		const current = store.current;
		if (command === 'volume') store.setVolume(Number(value));
		if (!current) return;
		if (command === 'play') store.resume();
		else if (command === 'pause') store.pause();
		else if (command === 'toggle') store.toggle();
		else if (command === 'next') step(1);
		else if (command === 'previous') step(-1);
		else if (command === 'seek') {
			// As this device reports it: within the track for music, within the whole otherwise.
			if (current.kind === 'album') store.seekInTrack(Number(value) || 0);
			else store.seek(Number(value) || 0);
		} else if (command === 'stop') {
			// Playback moved to another device: save the place, end the session, clear the bar.
			queue = null;
			void store.close();
		}
	}

	/** The user's own station `id` (name, logo), or null. */
	async function findStation(id) {
		try {
			const res = await fetch('/radio/stations');
			if (!res.ok) return null;
			return ((await res.json()).stations ?? []).find((station) => station.id === id) ?? null;
		} catch {
			return null;
		}
	}

	/**
	 * `take_over`: play here what another device played, where it stood. If the browser refuses
	 * to start sound without a click, everything is loaded and the mini bar offers "Hier
	 * weiterhören" (`current.refused`, set by the store).
	 */
	async function takeOverHere({ state, play }) {
		const plan = takeOverPlan(state);
		if (!plan) return;
		queue = null;
		let started = false;
		if (plan.kind === 'station') {
			const station = await findStation(plan.stationId);
			started = store.playStation({
				id: plan.stationId,
				name: station?.name ?? plan.title,
				hasLogo: Boolean(station?.has_logo)
			});
		} else if (plan.kind === 'item') {
			started = await playEntry({ itemId: plan.itemId, trackId: null, kind: plan.itemKind });
			if (started) store.seek(plan.position);
		} else {
			const first = plan.entries[0];
			queue = createQueue(REMOTE_QUEUE, plan.entries);
			await playQueueAt(0);
			started = isEntryPlaying(first, store.current);
			if (started) {
				// A whole album leads the list: go to the track that ran, then to the place in it.
				if (!first.trackId && plan.trackId) await store.playTrackById(first.itemId, plan.trackId);
				store.seekInTrack(plan.position);
			}
		}
		if (started && play === false) store.pause();
		if (store.current?.itemId) followFullscreen(store.current.itemId);
		void link?.reportNow();
	}

	/** While the mini bar is another device's remote, its buttons go there. */
	function remoteOr(command, local) {
		return () => {
			const remote = remoteView();
			if (remote) void link.send(remote.device.id, command);
			else local();
		};
	}

	function showDevicesError(reason) {
		devicesError = reason ?? '';
		// Without the panel there is nowhere else to say it.
		if (devicesError && devicesPanel) devicesPanel.hidden = false;
		scheduleRender();
	}

	/** "Hier weiterhören": what the watched device plays moves to this browser. */
	async function listenHere(fromId) {
		if (!link) return;
		// The hand-over must arrive in this tab, not in another one of this browser.
		if (!(await link.hold())) {
			showDevicesError('Keine Verbindung zum Server');
			return;
		}
		showDevicesError(await link.transfer(link.selfId, fromId));
	}

	/** A row of the devices panel: playback moves to that device. */
	async function pickDevice(id) {
		if (!link || !id) return;
		devicesError = '';
		if (id === link.selfId) {
			// Already here: at most it waits for the click the browser wanted.
			if (store.current) {
				if (store.current.refused) store.resume();
				return;
			}
			await listenHere(null);
			return;
		}
		if (!store.current) {
			showDevicesError(await link.transfer(id, null));
			return;
		}
		// From here: the server hands over what this tab reported, so it has to be the tab
		// that reports, and up to date.
		await link.hold();
		await link.reportNow();
		showDevicesError(await link.transfer(id, link.selfId));
	}

	function toggleDevicesPanel(open) {
		if (!devicesPanel || !link) return;
		devicesPanel.hidden = !open;
		devicesError = '';
		if (open) void link.refresh();
		scheduleRender();
	}

	const ACTIONS = {
		toggle: remoteOr('toggle', () => store.toggle()),
		'skip-back': () => store.skipBack(),
		'skip-forward': () => store.skipForward(),
		previous: remoteOr('previous', () => step(-1)),
		next: remoteOr('next', () => step(1)),
		here: () => {
			if (store.current) store.resume();
			else void listenHere(remoteView()?.device.id ?? null);
		},
		devices: () => toggleDevicesPanel(Boolean(devicesPanel?.hidden)),
		'devices-close': () => toggleDevicesPanel(false),
		device: (el) => void pickDevice(el.dataset.deviceId),
		'nudge-back': () => store.nudge(-1),
		'nudge-forward': () => store.nudge(1),
		'jump-previous': () => step(-1),
		'jump-next': () => step(1),
		play: (el) => void store.play(pageItemId(el)),
		restart: (el) => void restart(pageItemId(el)),
		shuffle: (el) => void store.play(pageItemId(el), undefined, { shuffle: true }),
		'play-track': (el) => void store.playTrackById(pageItemId(el), Number(el.dataset.trackId)),
		'play-from': (el) => void store.playFrom(pageItemId(el), Number(el.dataset.position)),
		// A line of the lyrics: its time is within the track that plays.
		'seek-lyric': (el) => store.seekInTrack(Number(el.dataset.position)),
		bookmark: (el) => void addBookmark(el),
		'play-episode': (el) => void playEpisode(el),
		'download-episode': (el) => void downloadEpisode(Number(el.dataset.episodeId), el),
		'play-playlist': (el) => startPlaylist(el, 0),
		'play-entry': (el) => startPlaylist(el, Number(el.dataset.entryIndex) || 0),
		radio: (el) => radioFrom(el),
		'radio-playing': () => radioFromPlaying(),
		'play-station': (el) => playStation(el),
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
			// A click anywhere else puts the devices panel away.
			if (
				devicesPanel &&
				!devicesPanel.hidden &&
				event.target instanceof Element &&
				!event.target.closest('[data-devices-panel], [data-action="devices"]')
			) {
				toggleDevicesPanel(false);
			}
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
		// The volume sliders of the remote control: another device's in the mini bar, this
		// browser's own in the devices panel. On release, not for every step of a drag.
		if (target instanceof HTMLInputElement && target.matches('[data-remote-volume]')) {
			const remote = remoteView();
			if (remote) void link.send(remote.device.id, 'volume', Number(target.value));
			return;
		}
		if (target instanceof HTMLInputElement && target.matches('[data-own-volume]')) {
			store.setVolume(Number(target.value));
			return;
		}
		if (target instanceof HTMLInputElement && target.matches('[data-autoplay-toggle]')) {
			setAutoplay(storage, target.checked);
			return;
		}
		if (target instanceof HTMLInputElement && target.matches('[data-normalize-toggle]')) {
			setNormalize(storage, target.checked);
			// Audible at once, also in the track that is playing.
			store.applyVolume();
			return;
		}
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
		// A radio station has neither progress nor a session to close.
		if (!current || current.live) return;
		const json = (body) => new Blob([JSON.stringify(body)], { type: 'application/json' });
		// A single track (playlist entry, radio) leaves its album's progress alone.
		if (!current.single) {
			navigator.sendBeacon(
				`/progress/${current.itemId}`,
				json({ position: store.savedPosition(), finished: false })
			);
		}
		navigator.sendBeacon(`/sessions/${current.sessionId}/close`, json(store.closeBody()));
	});

	bindMediaSession(store, {
		previous: () => step(-1),
		next: () => step(1),
		fetchItem
	});

	mountPage();
}
