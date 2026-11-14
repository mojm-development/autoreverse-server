// The progress filter on the books page (legacy library/books/+page.svelte): Alle / Wird gehört /
// Nicht begonnen / Beendet, applied to the rendered list without a round trip.
document.addEventListener('click', (event) => {
	const button = event.target instanceof Element ? event.target.closest('[data-filter]') : null;
	const group = button?.closest('[data-status-filter]');
	if (!button || !group) return;
	const filter = button.dataset.filter;
	for (const option of group.querySelectorAll('[data-filter]')) {
		const active = option === button;
		option.classList.toggle('active', active);
		option.setAttribute('aria-pressed', String(active));
	}
	let shown = 0;
	for (const book of document.querySelectorAll('[data-status]')) {
		const match = filter === 'all' || book.dataset.status === filter;
		book.hidden = !match;
		if (match) shown += 1;
	}
	const empty = document.querySelector('[data-filter-empty]');
	if (empty) empty.hidden = shown > 0;
});
