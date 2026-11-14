// The artists page's search (legacy library/artists/+page.svelte): filters the tiles by name as
// you type and says so when nothing matches.
document.addEventListener('input', (event) => {
	const input = event.target instanceof Element ? event.target.closest('[data-artist-search]') : null;
	if (!input) return;
	const raw = input.value;
	const needle = raw.trim().toLowerCase();
	let shown = 0;
	for (const tile of document.querySelectorAll('[data-artist]')) {
		const match = !needle || tile.dataset.search.includes(needle);
		tile.hidden = !match;
		if (match) shown += 1;
	}
	const grid = document.querySelector('[data-artist-grid]');
	if (grid) grid.hidden = shown === 0;
	const empty = document.querySelector('[data-artist-empty]');
	if (empty) {
		empty.hidden = shown > 0;
		empty.textContent = `Kein Interpret passt zu „${raw}“.`;
	}
});
