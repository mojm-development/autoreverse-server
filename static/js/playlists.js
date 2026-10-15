// Playlists in the web UI: the "Zur Playlist hinzufügen" dialog of the album and book pages, and
// drag and drop on a playlist page (legacy library/playlists/[id]). Delegated listeners, so
// pages swapped in by htmx work without re-binding. Playing a playlist is static/js/player.

(function () {
	const JSON_HEADERS = { 'content-type': 'application/json' };

	/** What the dialog adds: `{item_id}` or `{track_id}`, set by the button that opened it. */
	let pending = null;

	function elementOf(event) {
		const target = event.target;
		if (target instanceof Element) return target;
		return target?.parentElement ?? null;
	}

	function setStatus(dialog, text) {
		const status = dialog.querySelector('[data-playlist-status]');
		if (status) status.textContent = text;
	}

	async function detailOf(res, fallback) {
		try {
			const body = await res.json();
			return typeof body?.detail === 'string' ? body.detail : fallback;
		} catch {
			return fallback;
		}
	}

	async function addTo(dialog, playlistId, name) {
		if (!pending) return;
		setStatus(dialog, 'Wird hinzugefügt …');
		try {
			const res = await fetch(`/playlists/${playlistId}/entries`, {
				method: 'POST',
				headers: JSON_HEADERS,
				body: JSON.stringify(pending)
			});
			if (!res.ok) {
				setStatus(dialog, await detailOf(res, 'Hinzufügen fehlgeschlagen.'));
				return;
			}
			setStatus(dialog, `Zu „${name}“ hinzugefügt.`);
		} catch {
			setStatus(dialog, 'Keine Verbindung zum Server.');
		}
	}

	function choiceButton(id, name) {
		const item = document.createElement('li');
		const button = document.createElement('button');
		button.type = 'button';
		button.className = 'choice';
		button.dataset.playlistTarget = String(id);
		const label = document.createElement('span');
		label.textContent = name;
		button.append(label);
		item.append(button);
		return item;
	}

	document.addEventListener('click', (event) => {
		const target = elementOf(event);
		if (!target) return;

		const opener = target.closest('[data-playlist-pick]');
		if (opener) {
			const dialog = document.querySelector('[data-playlist-picker]');
			if (!dialog) return;
			event.preventDefault();
			pending = opener.dataset.pickTrack
				? { track_id: Number(opener.dataset.pickTrack) }
				: { item_id: Number(opener.dataset.pickItem) };
			setStatus(dialog, '');
			dialog.showModal();
			return;
		}

		const choice = target.closest('[data-playlist-target]');
		if (choice) {
			const dialog = choice.closest('[data-playlist-picker]');
			if (dialog) void addTo(dialog, choice.dataset.playlistTarget, choice.textContent.trim());
			return;
		}

		if (target.closest('[data-playlist-close]')) {
			target.closest('dialog')?.close();
		}
	});

	// "Anlegen" in the dialog: create the playlist, list it, add to it.
	document.addEventListener('submit', (event) => {
		const form = elementOf(event)?.closest('[data-playlist-new]');
		if (!form) return;
		event.preventDefault();
		const dialog = form.closest('[data-playlist-picker]');
		const input = form.querySelector('input[name="name"]');
		const name = input?.value.trim() ?? '';
		if (!dialog || !name) return;
		void (async () => {
			try {
				const res = await fetch('/playlists', {
					method: 'POST',
					headers: JSON_HEADERS,
					body: JSON.stringify({ name })
				});
				if (!res.ok) {
					setStatus(dialog, await detailOf(res, 'Anlegen fehlgeschlagen.'));
					return;
				}
				const playlist = await res.json();
				dialog.querySelector('[data-playlist-choices]')?.append(choiceButton(playlist.id, playlist.name));
				input.value = '';
				await addTo(dialog, playlist.id, playlist.name);
			} catch {
				setStatus(dialog, 'Keine Verbindung zum Server.');
			}
		})();
	});

	// Drag and drop on a playlist page: drop a row onto another to move it to that position.
	let dragged = null;

	function rowOf(event) {
		return elementOf(event)?.closest('[data-entry-id]') ?? null;
	}

	function clearMarks() {
		for (const row of document.querySelectorAll('.drop-target, .dragging')) {
			row.classList.remove('drop-target', 'dragging');
		}
	}

	/** Swaps in the entry list of a fresh copy of the page; the player and the rest stay. */
	async function refreshEntries() {
		const res = await fetch(location.href, { headers: { accept: 'text/html' } });
		if (!res.ok) return;
		const fresh = new DOMParser().parseFromString(await res.text(), 'text/html');
		const next = fresh.querySelector('[data-refresh="playlist-entries"]');
		const current = document.querySelector('[data-refresh="playlist-entries"]');
		if (next && current) current.innerHTML = next.innerHTML;
	}

	document.addEventListener('dragstart', (event) => {
		const row = rowOf(event);
		if (!row) return;
		dragged = row;
		row.classList.add('dragging');
		if (event.dataTransfer) {
			event.dataTransfer.effectAllowed = 'move';
			event.dataTransfer.setData('text/plain', row.dataset.entryId);
		}
	});

	document.addEventListener('dragover', (event) => {
		const row = rowOf(event);
		if (!row || !dragged || row.parentElement !== dragged.parentElement) return;
		event.preventDefault();
		if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
		for (const other of document.querySelectorAll('.drop-target')) {
			if (other !== row) other.classList.remove('drop-target');
		}
		if (row !== dragged) row.classList.add('drop-target');
	});

	document.addEventListener('dragend', () => {
		dragged = null;
		clearMarks();
	});

	document.addEventListener('drop', (event) => {
		const row = rowOf(event);
		const source = dragged;
		dragged = null;
		clearMarks();
		if (!row || !source || row === source || row.parentElement !== source.parentElement) return;
		event.preventDefault();
		const playlistId = row.closest('[data-playlist-id]')?.dataset.playlistId;
		if (!playlistId) return;
		void (async () => {
			try {
				const res = await fetch(`/playlists/${playlistId}/entries/${source.dataset.entryId}`, {
					method: 'PUT',
					headers: JSON_HEADERS,
					body: JSON.stringify({ position: Number(row.dataset.entryPosition) })
				});
				if (res.ok || res.status === 404) await refreshEntries();
			} catch {
				// Offline: the order stays as it was.
			}
		})();
	});
})();
