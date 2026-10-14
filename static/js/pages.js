// Small page behaviours that need the browser rather than the server: the start page's greeting
// by the visitor's clock, and the album list's infinite scroll (status line, no doubled tiles).
// The decisions are plain functions so tests/js can check them without a DOM.

/** legacy library/+page.svelte: by the hour on the visitor's clock, not the server's. */
export function greetingFor(hour) {
	if (hour < 12) return 'Guten Morgen';
	if (hour < 18) return 'Guten Tag';
	return 'Guten Abend';
}

/**
 * Indexes of ids already seen earlier in the list. Offset paging shifts by one when an album is
 * added while scrolling, and the next page then repeats the last tile of the previous one
 * (legacy filtered known ids out of each page).
 */
export function laterDuplicates(ids) {
	const seen = new Set();
	const doubled = [];
	ids.forEach((id, index) => {
		if (seen.has(id)) doubled.push(index);
		else seen.add(id);
	});
	return doubled;
}

function renderGreeting() {
	for (const heading of document.querySelectorAll('[data-greeting]')) {
		const text = `${greetingFor(new Date().getHours())}, ${heading.dataset.greeting}`;
		if (heading.textContent !== text) heading.textContent = text;
	}
}

/** Removes the second copy of any tile or row in a `[data-paged-list]`. */
function dropDoubledEntries() {
	for (const list of document.querySelectorAll('[data-paged-list]')) {
		const entries = [...list.querySelectorAll(':scope > [data-entry-key]')];
		for (const index of laterDuplicates(entries.map((e) => e.dataset.entryKey))) {
			entries[index].remove();
		}
	}
}

function statusFor(element) {
	return element?.closest('[data-paged-root]')?.querySelector('[data-paged-status]') ?? null;
}

if (typeof document !== 'undefined') {
	renderGreeting();
	document.addEventListener('htmx:load', renderGreeting);
	// InfiniteScroll.svelte: "Weitere Alben werden geladen…" while the next page is on its way.
	document.addEventListener('htmx:beforeRequest', (event) => {
		const sentinel = event.target instanceof Element ? event.target.closest('.sentinel') : null;
		const status = statusFor(sentinel);
		if (status) status.textContent = `${sentinel.dataset.loadingLabel ?? 'Weitere Einträge werden geladen'}…`;
	});
	document.addEventListener('htmx:afterRequest', (event) => {
		const sentinel = event.target instanceof Element ? event.target.closest('.sentinel') : null;
		if (!sentinel) return;
		const status = statusFor(sentinel) ?? document.querySelector('[data-paged-status]');
		if (status) status.textContent = '';
	});
	document.addEventListener('htmx:afterSettle', dropDoubledEntries);
}
