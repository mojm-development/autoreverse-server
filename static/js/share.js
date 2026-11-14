// Sharing in the web UI: "Link kopieren" on the album, book, podcast, artist and playlist pages,
// the row a link to a single track points at, and "Importieren" on the playlists page (an M3U
// file read here and sent to POST /playlists/import as text).
// A copied link leads to a page of this server: whoever opens it needs an account here.
// The decisions are plain functions so tests/js can check them without a DOM.

/** The address to hand on for a page path: under the origin the visitor reached the server by. */
export function absoluteLink(path, origin) {
	return new URL(path, origin).href;
}

/**
 * The name an imported playlist gets: the file's own `#PLAYLIST:` line, else the file name
 * without its extension. At most the 100 UTF-16 units the server takes.
 */
export function importName(fileName, content) {
	const tagged = /^#PLAYLIST:(.*)$/im.exec(content ?? '')?.[1]?.trim();
	const fromFile = (fileName ?? '').replace(/\.(m3u8?|txt)$/i, '').trim();
	return (tagged || fromFile || 'Importierte Playlist').slice(0, 100);
}

/** The line above the list of what an import did not find. */
export function importSummary(name, matched, unmatched) {
	const total = matched + unmatched;
	return unmatched === 0
		? `„${name}“ angelegt: alle ${total} Titel gefunden.`
		: `„${name}“ angelegt: ${matched} von ${total} Titeln gefunden.`;
}

/** The largest file the server takes (its limit counts bytes of text). */
export const MAX_IMPORT_BYTES = 2 * 1024 * 1024;

async function copyText(text) {
	try {
		await navigator.clipboard.writeText(text);
		return true;
	} catch {
		// No clipboard API without HTTPS (a server reached by its LAN address): the old way.
		const field = document.createElement('textarea');
		field.value = text;
		field.setAttribute('readonly', '');
		field.style.position = 'fixed';
		field.style.opacity = '0';
		document.body.append(field);
		field.select();
		let done = false;
		try {
			done = document.execCommand('copy');
		} catch {
			done = false;
		}
		field.remove();
		return done;
	}
}

async function copyLink(button) {
	const link = absoluteLink(button.dataset.copyLink, location.origin);
	const label = button.dataset.copyLabel ?? button.textContent;
	button.dataset.copyLabel = label;
	button.textContent = (await copyText(link)) ? 'Link kopiert' : 'Kopieren nicht möglich';
	setTimeout(() => {
		button.textContent = label;
	}, 2000);
}

/** Brings the row a `?track=` link points at into view. */
function showLinkedTrack() {
	document.querySelector('[data-linked-track]')?.scrollIntoView({ block: 'center' });
}

function line(tag, text) {
	const element = document.createElement(tag);
	element.textContent = text;
	return element;
}

/** Shows what an import did: a sentence, the playlist's link, and what was not found. */
function showImport(box, summary, playlistId, unmatched) {
	box.replaceChildren(line('p', summary));
	if (playlistId != null) {
		const link = line('a', 'Playlist öffnen');
		link.href = `/library/playlists/${playlistId}`;
		box.append(link);
	}
	if (unmatched.length > 0) {
		const details = document.createElement('details');
		details.open = true;
		details.append(line('summary', `Nicht gefunden (${unmatched.length})`));
		const list = document.createElement('ul');
		for (const entry of unmatched) list.append(line('li', entry));
		details.append(list);
		box.append(details);
	}
	box.hidden = false;
	// The link was added after htmx looked at the page.
	window.htmx?.process(box);
}

/** Swaps in the tiles of a fresh copy of the page; the player and the rest stay. */
async function refreshTiles() {
	const res = await fetch(location.href, { headers: { accept: 'text/html' } });
	if (!res.ok) return;
	const fresh = new DOMParser().parseFromString(await res.text(), 'text/html');
	const next = fresh.querySelector('[data-refresh="playlists"]');
	const current = document.querySelector('[data-refresh="playlists"]');
	if (next && current) {
		current.innerHTML = next.innerHTML;
		window.htmx?.process(current);
	}
}

async function importFile(input) {
	const file = input.files?.[0];
	const box = document.querySelector('[data-import-result]');
	input.value = '';
	if (!file || !box) return;
	if (file.size > MAX_IMPORT_BYTES) {
		showImport(box, 'Die Datei ist zu groß (maximal 2 MB).', null, []);
		return;
	}
	showImport(box, 'Wird importiert …', null, []);
	try {
		const content = await file.text();
		const name = importName(file.name, content);
		const res = await fetch('/playlists/import', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ name, content })
		});
		const body = await res.json().catch(() => ({}));
		const unmatched = Array.isArray(body.unmatched) ? body.unmatched : [];
		if (!res.ok) {
			showImport(box, body.detail ?? 'Importieren fehlgeschlagen.', null, unmatched);
			return;
		}
		showImport(box, importSummary(body.playlist.name, body.matched, unmatched.length), body.playlist.id, unmatched);
		await refreshTiles();
	} catch {
		showImport(box, 'Keine Verbindung zum Server.', null, []);
	}
}

if (typeof document !== 'undefined') {
	// Delegated, so pages swapped in by htmx work without re-binding.
	document.addEventListener('click', (event) => {
		const target = event.target instanceof Element ? event.target : null;
		const copy = target?.closest('[data-copy-link]');
		if (copy) {
			event.preventDefault();
			void copyLink(copy);
			return;
		}
		if (target?.closest('[data-playlist-import-open]')) {
			document.querySelector('[data-playlist-import]')?.click();
		}
	});
	document.addEventListener('change', (event) => {
		const input = event.target instanceof Element ? event.target.closest('[data-playlist-import]') : null;
		if (input) void importFile(input);
	});
	showLinkedTrack();
	document.addEventListener('htmx:afterSettle', showLinkedTrack);
}
