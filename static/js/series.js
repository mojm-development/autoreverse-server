// The series page's search (legacy library/series/+page.svelte): filters the shelves by series
// or author as you type, hides author groups left empty and says so when nothing matches.
document.addEventListener('input', (event) => {
	const input = event.target instanceof Element ? event.target.closest('[data-series-search]') : null;
	if (!input) return;
	const raw = input.value;
	const needle = raw.trim().toLowerCase();
	let shown = 0;
	for (const group of document.querySelectorAll('[data-series-group]')) {
		let visible = 0;
		for (const entry of group.querySelectorAll('[data-series-entry]')) {
			const match = !needle || entry.dataset.search.includes(needle);
			entry.hidden = !match;
			if (match) visible += 1;
		}
		group.hidden = visible === 0;
		const count = group.querySelector('[data-series-count]');
		if (count) count.textContent = String(visible);
		shown += visible;
	}
	const empty = document.querySelector('[data-series-empty]');
	if (empty) {
		empty.hidden = shown > 0;
		empty.textContent = `Keine Serie passt zu „${raw}“.`;
	}
});
