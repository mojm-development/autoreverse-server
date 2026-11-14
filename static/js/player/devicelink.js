// This browser as a device of the remote control: the event stream from the server
// (`EventSource` on `GET /me/devices/events`), the reports of what plays here, and the requests
// that steer other devices. The rules it follows are in devices.js.

import {
	BEACON_MS,
	createElection,
	deviceId,
	deviceName,
	localState,
	shouldReport
} from './devices.js';

const CHANNEL = 'autoreverse-devices';
/** The store notifies several times a second while it plays; that often nobody builds a state. */
const REPORT_THROTTLE_MS = 400;
/** After a report the server refused or never got, the next try waits this long. */
const REPORT_RETRY_MS = 5_000;
/** A stream the server closed for good (logged out) is tried again this much later. */
const REOPEN_MS = 30_000;

/**
 * @param {object} options
 * @param {ReturnType<import('./store.js').createPlayerStore>} options.store
 * @param {() => object | null} options.getQueue The playlist or radio that plays (queue.js).
 * @param {(itemId: number) => object | null | undefined} options.itemInfo The `/items/{id}`
 *   answer if it is there already; asking for it is the caller's business.
 * @param {() => void} options.onDevices The list changed.
 * @param {(command: {command: string, value: unknown, from: string}) => void} options.onCommand
 * @param {(takeOver: {state: object, from: string | null, play: boolean}) => void} options.onTakeOver
 */
export function createDeviceLink({ store, getQueue, itemInfo, onDevices, onCommand, onTakeOver }) {
	const storage = {
		getItem: (key) => localStorage.getItem(key),
		setItem: (key, value) => localStorage.setItem(key, value)
	};
	const selfId = deviceId(storage);
	const name = deviceName(navigator.userAgent);
	const tabId = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
	const channel = globalThis.BroadcastChannel ? new BroadcastChannel(CHANNEL) : null;

	let devices = [];
	let source = null;
	let reopenTimer = null;
	/** `{state, at}` of the last report the server took, see shouldReport. */
	let sent = null;
	let reportTimer = null;
	let retryAfter = 0;
	let wasPlaying = false;
	/** Callers waiting for the stream to be open (see `hold`). */
	let waiting = [];

	function post(message) {
		try {
			channel?.postMessage(message);
		} catch {
			// A closed channel (the page is going away): nobody left to tell.
		}
	}

	const election = createElection({
		tabId,
		post,
		isPlaying: () => Boolean(store.current?.playing),
		isVisible: () => document.visibilityState === 'visible',
		onChange: (holding) => (holding ? open() : close())
	});

	function setDevices(next) {
		devices = Array.isArray(next) ? next : [];
		onDevices();
	}

	function close() {
		if (reopenTimer) clearTimeout(reopenTimer);
		reopenTimer = null;
		source?.close();
		source = null;
	}

	function open() {
		close();
		const query = new URLSearchParams({ device_id: selfId, name, kind: 'web' });
		const stream = new EventSource(`/me/devices/events?${query}`);
		source = stream;
		const json = (handler) => (event) => {
			if (source !== stream) return;
			try {
				handler(JSON.parse(event.data));
			} catch {
				// An event this version does not understand.
			}
		};
		stream.addEventListener('open', () => {
			if (source !== stream) return;
			// A new stream may be a new server that knows nothing of this device yet.
			sent = null;
			retryAfter = 0;
			scheduleReport(0);
			for (const resolve of waiting.splice(0)) resolve(true);
		});
		stream.addEventListener(
			'devices',
			json((data) => {
				setDevices(data.devices);
				// The tabs without a stream show the same list.
				post({ t: 'devices', tab: tabId, devices });
			})
		);
		stream.addEventListener('command', json(onCommand));
		stream.addEventListener('take_over', json(onTakeOver));
		stream.addEventListener('error', () => {
			// While it is CONNECTING the browser retries by itself. CLOSED means the server
			// answered with an error (no session any more): ask again much later, not in a loop.
			if (source !== stream || stream.readyState !== EventSource.CLOSED) return;
			source = null;
			reopenTimer = setTimeout(() => {
				if (election.holding) open();
			}, REOPEN_MS);
		});
	}

	if (channel) {
		channel.onmessage = (event) => {
			const message = event.data;
			if (message?.t === 'devices') {
				if (!election.holding) setDevices(message.devices);
				return;
			}
			election.receive(message);
			// A new tab asked who holds the stream: it wants the list as well.
			if (message?.t === 'who' && election.holding) post({ t: 'devices', tab: tabId, devices });
		};
	}

	async function report() {
		reportTimer = null;
		if (!election.holding || source?.readyState !== EventSource.OPEN) return;
		const current = store.current;
		const now = Date.now();
		if (now < retryAfter) return;
		const state = localState({
			current,
			queue: getQueue(),
			item: current?.itemId ? itemInfo(current.itemId) : null,
			volume: store.volume,
			now
		});
		if (!shouldReport(sent, state, now)) return;
		const before = sent;
		sent = { state, at: now };
		try {
			const res = await fetch(`/me/devices/${selfId}/state`, {
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(state)
			});
			if (res.ok) return;
		} catch {
			// Offline for a moment.
		}
		// Not taken: what the server knows is still the report before.
		if (sent?.state === state) sent = before;
		retryAfter = Date.now() + REPORT_RETRY_MS;
	}

	function scheduleReport(delay = REPORT_THROTTLE_MS) {
		if (reportTimer) {
			if (delay > 0) return;
			clearTimeout(reportTimer);
		}
		reportTimer = setTimeout(() => void report(), delay);
	}

	store.subscribe(() => {
		const playing = Boolean(store.current?.playing);
		// Starting to play makes this the tab that answers for the device.
		if (playing && !wasPlaying) election.wantHold();
		wasPlaying = playing;
		if (election.holding) scheduleReport();
	});

	setInterval(() => election.tick(), BEACON_MS);
	document.addEventListener('visibilitychange', () => {
		if (document.visibilityState === 'visible') election.tick();
	});
	// No sign-off here: a reload or a full page change comes back within the server's grace
	// period, and nobody sees the device flicker. Another open tab takes the stream over.
	window.addEventListener('pagehide', () => {
		election.leave();
		close();
	});
	// Back from the back/forward cache, with the stream closed above.
	window.addEventListener('pageshow', (event) => {
		if (event.persisted) election.start();
	});
	election.start();

	async function request(path, body) {
		try {
			const res = await fetch(path, {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(body)
			});
			if (res.ok) return null;
			const answer = await res.json().catch(() => null);
			return answer?.detail ?? 'Das hat nicht geklappt';
		} catch {
			return 'Der Server ist nicht erreichbar';
		}
	}

	return {
		selfId,
		get devices() {
			return devices;
		},
		/** Asks the server for the list (a tab without the stream, a panel just opened). */
		async refresh() {
			try {
				const res = await fetch('/me/devices');
				if (res.ok) setDevices((await res.json()).devices);
			} catch {
				// The list stays as it was.
			}
		},
		/** Reports what plays here without waiting for the next round; resolves once it is said. */
		reportNow() {
			if (reportTimer) clearTimeout(reportTimer);
			return report();
		},
		/**
		 * Makes this tab the one with the stream and waits until it is open: what is handed
		 * over to this browser must arrive in the tab that asked for it. False after three
		 * seconds without a stream.
		 */
		hold() {
			election.wantHold();
			if (source?.readyState === EventSource.OPEN) return Promise.resolve(true);
			return new Promise((resolve) => {
				waiting.push(resolve);
				setTimeout(() => {
					waiting = waiting.filter((entry) => entry !== resolve);
					resolve(false);
				}, 3000);
			});
		},
		/** A command for another device; resolves to null, or to the German reason it failed. */
		send: (targetId, command, value = null) =>
			request(`/me/devices/${targetId}/commands`, { command, value, from: selfId }),
		/** Hands playback from `from` (null: the active device) to `targetId`; null or the reason. */
		transfer: (targetId, from = null) => request(`/me/devices/${targetId}/transfer`, { from, play: true })
	};
}
