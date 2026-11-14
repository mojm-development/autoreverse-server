// The list of the account's devices (the panel behind the "Geräte" buttons). Built with DOM
// calls only: device names and titles come from other clients and are text, never markup.

import { kindLabel, mediaLine } from './devices.js';

/** What a row says under the device's name. */
export function deviceLine(device) {
	const state = device.state;
	const kind = kindLabel(device.kind);
	if (!state?.media) return `${kind} · bereit`;
	return `${kind} · ${state.playing ? 'spielt' : 'pausiert'}: ${mediaLine(state.media)}`;
}

function row(device, selfId) {
	const item = document.createElement('li');
	const button = document.createElement('button');
	button.type = 'button';
	button.className = 'device';
	button.dataset.action = 'device';
	button.dataset.deviceId = device.id;
	button.setAttribute('aria-current', String(Boolean(device.active)));
	const own = device.id === selfId;
	button.title = own ? 'Hier weiterhören' : `Auf ${device.name} weiterhören`;
	const name = document.createElement('span');
	name.className = 'name';
	name.textContent = device.name;
	button.append(name);
	if (own) {
		const badge = document.createElement('span');
		badge.className = 'badge';
		badge.textContent = 'Dieses Gerät';
		button.append(badge);
	}
	const what = document.createElement('span');
	what.className = 'what';
	what.textContent = deviceLine(device);
	button.append(what);
	item.append(button);
	return item;
}

/**
 * Draws the open panel: this device first, then the others. The rows are only rebuilt when
 * what they show changed — a rebuild under the pointer would swallow the click.
 */
export function renderDevicesPanel(panel, { devices, selfId, error, volume }) {
	if (!panel || panel.hidden) return;
	const list = panel.querySelector('[data-devices-list]');
	const ordered = [...devices].sort((a, b) => Number(b.id === selfId) - Number(a.id === selfId));
	const key = JSON.stringify(ordered.map((device) => [device.id, device.name, device.active, deviceLine(device)]));
	if (list && list.dataset.key !== key) {
		list.dataset.key = key;
		list.replaceChildren(...ordered.map((device) => row(device, selfId)));
	}
	const empty = panel.querySelector('[data-devices-empty]');
	if (empty) empty.hidden = ordered.length > 0;
	const message = panel.querySelector('[data-devices-error]');
	if (message) {
		message.hidden = !error;
		if (message.textContent !== error) message.textContent = error;
	}
	const own = panel.querySelector('[data-own-volume]');
	if (own && document.activeElement !== own) own.value = String(volume);
}
