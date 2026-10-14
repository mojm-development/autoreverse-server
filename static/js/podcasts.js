// Podcast pages: the directory search with "Abonnieren" (legacy PodcastSearch.svelte), the
// episode filter pills and the two-step "Abo kündigen" (legacy library/podcasts/[id]).
// Delegated listeners, so pages swapped in by htmx work without re-binding. Playing and
// downloading episodes is static/js/player.

(function () {
	const JSON_HEADERS = { 'content-type': 'application/json' };

	function elementOf(event) {
		const target = event.target;
		if (target instanceof Element) return target;
		return target?.parentElement ?? null;
	}

	/** `background-image` for a directory artwork URL, quotes and backslashes escaped. */
	function coverStyle(url) {
		if (!url) return '';
		const safe = url.replaceAll('\\', '%5C').replaceAll('"', '%22');
		return `background-image: url("${safe}")`;
	}

	function show(element, text) {
		if (!element) return;
		element.textContent = text ?? element.textContent;
		element.hidden = false;
	}

	/** The `?q=` of the page follows the search, so coming back to it is not a blank slate. */
	function syncUrl(query) {
		const url = new URL(location.href);
		if (query.trim()) url.searchParams.set('q', query);
		else url.searchParams.delete('q');
		history.replaceState(history.state, '', url);
	}

	/** Re-reads the page and swaps in the subscription rail, so a new Abo shows up at once. */
	async function refreshRail() {
		const rail = document.querySelector('[data-refresh="podcast-rail"]');
		if (!rail) return;
		try {
			const res = await fetch(location.href, { headers: { accept: 'text/html' } });
			if (!res.ok) return;
			const fresh = new DOMParser().parseFromString(await res.text(), 'text/html');
			const next = fresh.querySelector('[data-refresh="podcast-rail"]');
			if (!next) return;
			rail.innerHTML = next.innerHTML;
			window.htmx?.process(rail);
		} catch {
			// The rail stays as it was; the next page load shows the new Abo.
		}
	}

	function resultRow(result, query, subscribedId) {
		const item = document.createElement('li');
		const open = document.createElement('a');
		open.className = 'open';
		open.href = `/library/podcasts/preview?feed=${encodeURIComponent(result.feed_url)}&q=${encodeURIComponent(query)}`;
		const cover = document.createElement('span');
		cover.className = 'cover';
		cover.setAttribute('style', coverStyle(result.artwork_url));
		const meta = document.createElement('span');
		meta.className = 'meta';
		const name = document.createElement('span');
		name.className = 'name';
		name.textContent = result.name;
		meta.append(name);
		if (result.author) {
			const author = document.createElement('span');
			author.className = 'author';
			author.textContent = result.author;
			meta.append(author);
		}
		if (result.episode_count !== null && result.episode_count !== undefined) {
			const count = document.createElement('span');
			count.className = 'episodes-count mono';
			count.textContent = `${result.episode_count} Folgen`;
			meta.append(count);
		}
		open.append(cover, meta);
		item.append(open, subscribeControl(result.feed_url, subscribedId));
		return item;
	}

	function subscribeControl(feedUrl, subscribedId) {
		if (subscribedId) {
			const link = document.createElement('a');
			link.className = 'secondary small subscribed';
			link.href = `/library/podcasts/${subscribedId}`;
			link.textContent = 'Abonniert ✓';
			return link;
		}
		const button = document.createElement('button');
		button.type = 'button';
		button.className = 'secondary small';
		button.dataset.podcastSubscribe = feedUrl;
		button.textContent = 'Abonnieren';
		return button;
	}

	async function search(root) {
		const input = root.querySelector('input[name="q"]');
		const list = root.querySelector('[data-podcast-search-results]');
		const error = root.querySelector('[data-podcast-search-error]');
		const empty = root.querySelector('[data-podcast-search-empty]');
		const button = root.querySelector('[data-podcast-search-form] button');
		if (!input || !list) return;
		const query = input.value;
		list.replaceChildren();
		if (error) error.hidden = true;
		if (empty) empty.hidden = true;
		syncUrl(query);
		if (!query.trim()) return;
		if (button) {
			button.disabled = true;
			button.textContent = '…';
		}
		try {
			const res = await fetch(`/podcasts/search?q=${encodeURIComponent(query)}`);
			const body = await res.json().catch(() => ({}));
			if (!res.ok) {
				show(error, body.detail ?? 'Suche fehlgeschlagen.');
				return;
			}
			const results = Array.isArray(body.results) ? body.results : [];
			if (results.length === 0) show(empty);
			list.replaceChildren(...results.map((r) => resultRow(r, query, null)));
			window.htmx?.process(list);
		} catch {
			show(error, 'Suche fehlgeschlagen.');
		} finally {
			if (button) {
				button.disabled = false;
				button.textContent = 'Suchen';
			}
		}
	}

	async function subscribe(button) {
		const root = button.closest('[data-podcast-search]');
		const error = root?.querySelector('[data-podcast-search-error]');
		const feedUrl = button.dataset.podcastSubscribe;
		button.disabled = true;
		button.textContent = '…';
		try {
			const res = await fetch('/podcasts', {
				method: 'POST',
				headers: JSON_HEADERS,
				body: JSON.stringify({ feed_url: feedUrl })
			});
			const body = await res.json().catch(() => ({}));
			if (!res.ok) {
				show(error, body.detail ?? 'Abonnieren fehlgeschlagen.');
				button.disabled = false;
				button.textContent = 'Abonnieren';
				return;
			}
			const link = subscribeControl(feedUrl, body.id);
			button.replaceWith(link);
			window.htmx?.process(link);
			void refreshRail();
		} catch {
			show(error, 'Abonnieren fehlgeschlagen.');
			button.disabled = false;
			button.textContent = 'Abonnieren';
		}
	}

	/** Filter pills on a podcast page: all, unheard, started, downloaded. */
	function applyFilter(pill) {
		const page = pill.closest('[data-podcast-page]');
		if (!page) return;
		const key = pill.dataset.episodeFilter;
		for (const other of page.querySelectorAll('[data-episode-filter]')) {
			const on = other === pill;
			other.classList.toggle('active', on);
			other.setAttribute('aria-pressed', String(on));
		}
		let visible = 0;
		for (const episode of page.querySelectorAll('[data-episode]')) {
			const match =
				key === 'all' ||
				(key === 'downloaded'
					? episode.dataset.downloaded === 'true'
					: episode.dataset.status === key);
			episode.hidden = !match;
			if (match) visible += 1;
		}
		const empty = page.querySelector('[data-filter-empty]');
		if (empty) empty.hidden = visible > 0;
	}

	document.addEventListener('submit', (event) => {
		const form = elementOf(event)?.closest('[data-podcast-search-form]');
		if (form) {
			event.preventDefault();
			event.stopPropagation();
			const root = form.closest('[data-podcast-search]');
			if (root) void search(root);
			return;
		}
		// "Aktualisieren" may take a while (feed plus downloads): say so.
		const refresh = elementOf(event)?.closest('[data-podcast-refresh]');
		const label = refresh?.querySelector('[data-refresh-label]');
		if (label) label.textContent = 'Wird geprüft …';
	}, true);

	document.addEventListener('click', (event) => {
		const target = elementOf(event);
		const subscribeButton = target?.closest('[data-podcast-subscribe]');
		if (subscribeButton) {
			event.preventDefault();
			void subscribe(subscribeButton);
			return;
		}
		const pill = target?.closest('[data-episode-filter]');
		if (pill) {
			applyFilter(pill);
			return;
		}
		const ask = target?.closest('[data-unsubscribe-ask]');
		if (ask) {
			const confirm = ask.parentElement?.querySelector('[data-unsubscribe-confirm]');
			if (confirm) {
				ask.hidden = true;
				confirm.hidden = false;
			}
			return;
		}
		const cancel = target?.closest('[data-unsubscribe-cancel]');
		if (cancel) {
			const confirm = cancel.closest('[data-unsubscribe-confirm]');
			const askAgain = confirm?.parentElement?.querySelector('[data-unsubscribe-ask]');
			if (confirm) confirm.hidden = true;
			if (askAgain) askAgain.hidden = false;
		}
	});

	/** A search box seeded from ?q= searches right away. */
	function searchSeeded() {
		for (const root of document.querySelectorAll('[data-podcast-search]')) {
			if (root.dataset.searched) continue;
			root.dataset.searched = '1';
			const input = root.querySelector('input[name="q"]');
			if (input?.value.trim()) void search(root);
		}
	}
	document.addEventListener('DOMContentLoaded', searchSeeded);
	document.addEventListener('htmx:load', searchSeeded);
})();
