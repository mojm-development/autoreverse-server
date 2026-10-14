// The web player: one store for the whole visit, bound to the `<audio>` element of the player
// layout. Page actions are `[data-action]` buttons, handled here by delegation.

import { createPlayerStore } from './store.js';
import { bindPlayerShortcuts } from './shortcuts.js';
import { bindScrubbers } from './scrubber.js';
import { createVisualizers } from './visualizer.js';
import { installNavigation } from './navigation.js';
import { render } from './ui.js';

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
	const store = createPlayerStore({ preferences: lastServerPreferences ?? undefined });
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
			render(store);
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

	async function restart(itemId) {
		if (await store.play(itemId)) store.seek(0);
	}

	const ACTIONS = {
		toggle: () => store.toggle(),
		'skip-back': () => store.skipBack(),
		'skip-forward': () => store.skipForward(),
		previous: () => store.jumpPrevious(),
		next: () => store.jumpNext(),
		'nudge-back': () => store.nudge(-1),
		'nudge-forward': () => store.nudge(1),
		'jump-previous': () => store.jumpPrevious(),
		'jump-next': () => store.jumpNext(),
		play: (el) => void store.play(pageItemId(el)),
		restart: (el) => void restart(pageItemId(el)),
		shuffle: (el) => void store.play(pageItemId(el), undefined, { shuffle: true }),
		'play-track': (el) => void store.playTrackById(pageItemId(el), Number(el.dataset.trackId)),
		'play-from': (el) => void store.playFrom(pageItemId(el), Number(el.dataset.position)),
		close: () => history.back()
	};

	// Capture phase: a play button inside a tile link must act before htmx follows the link.
	document.addEventListener(
		'click',
		(event) => {
			const element = event.target instanceof Element ? event.target.closest('[data-action]') : null;
			const action = element && ACTIONS[element.dataset.action];
			if (!action || element.disabled) return;
			event.preventDefault();
			event.stopPropagation();
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
		else if (action === 'previous') store.jumpPrevious();
		else if (action === 'next') store.jumpNext();
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

	mountPage();
}
