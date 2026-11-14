// The "Entdecken" page: everything comes from /ai/* and, for the rows counted from the listening
// history, /me/on-repeat and /me/repeat-rewind. Text is only ever set through textContent,
// because names come from media files, podcast feeds and a language model.

(function () {
	const JSON_HEADERS = { 'content-type': 'application/json' };
	let feedTimer = null;

	function el(tag, className, text) {
		const node = document.createElement(tag);
		if (className) node.className = className;
		if (text !== undefined) node.textContent = text;
		return node;
	}

	function page() {
		return document.querySelector('[data-discover]');
	}

	async function call(path, options) {
		const res = await fetch(path, options);
		const body = await res.json().catch(() => null);
		if (!res.ok) throw new Error(body?.detail ?? 'Das hat nicht geklappt');
		return body;
	}

	function post(path, payload) {
		return call(path, { method: 'POST', headers: JSON_HEADERS, body: JSON.stringify(payload ?? {}) });
	}

	/** Runs a request while `status` says so; expensive buttons stay disabled until it ends. */
	async function run(button, status, work) {
		button.disabled = true;
		status.textContent = 'Einen Moment, das kann etwas dauern …';
		status.dataset.busy = '';
		let message = '';
		try {
			message = (await work()) ?? '';
		} catch (e) {
			message = e.message;
		} finally {
			button.disabled = false;
			delete status.dataset.busy;
			status.textContent = message;
		}
	}

	function clock(seconds) {
		const whole = Math.round(seconds);
		return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
	}

	const CUT = ' Die Sammlung ist sehr groß, berücksichtigt wurden die zuletzt ergänzten Alben.';

	function renderPlaylist(root, result) {
		const target = root.querySelector('[data-ai-playlist-result]');
		const head = el('div', 'head');
		const title = el('div');
		const total = result.tracks.reduce((sum, track) => sum + track.duration, 0);
		title.append(el('h3', '', result.name), el('span', 'meta', `${result.tracks.length} Titel · ${Math.round(total / 60)} Min.`));
		const buttons = el('div', 'buttons');
		buttons.append(playButton('play-playlist', 'Abspielen'), savePlaylistButton(result.name, result.tracks, 'secondary'));
		head.append(title, buttons);
		target.dataset.playlistId = 'discover-generated';
		target.replaceChildren(head, trackList(result.tracks));
	}

	function savePlaylistButton(name, tracks, className) {
		const save = el('button', className, 'Als Playlist speichern');
		save.type = 'button';
		save.addEventListener('click', async () => {
			save.disabled = true;
			save.textContent = 'Speichere …';
			try {
				const created = await post('/playlists', { name });
				for (const track of tracks) {
					await post(`/playlists/${created.id}/entries`, { track_id: track.id });
				}
				location.href = `/library/playlists/${created.id}`;
			} catch (e) {
				save.disabled = false;
				save.textContent = e.message;
			}
		});
		return save;
	}

	function trackList(tracks) {
		const list = el('ol', 'discover-tracks');
		tracks.forEach((track, index) => {
			const row = el('li');
			// The player reads these, as it does on a playlist page.
			row.dataset.entryId = `${index}`;
			row.dataset.entryItem = `${track.item_id}`;
			row.dataset.entryTrack = `${track.id}`;
			row.dataset.entryKind = 'album';
			const name = el('div', 'ellipsis');
			name.append(el('span', '', track.title), el('span', 'artist', ` · ${track.artist ?? track.item_title}`));
			row.append(el('span', 'number', String(index + 1)), name, el('span', 'time', clock(track.duration)));
			list.append(row);
		});
		return list;
	}

	function playButton(action, label) {
		const button = el('button', 'primary', label);
		button.type = 'button';
		button.dataset.action = action;
		return button;
	}

	function cover(id, size) {
		const image = el('img');
		image.src = `/items/${id}/cover?size=${size}`;
		image.alt = '';
		image.loading = 'lazy';
		return image;
	}

	function albumTile(album) {
		const link = el('a', 'discover-album');
		link.href = `/library/albums/${album.id}`;
		const art = el('div', 'art');
		if (album.has_cover) art.append(cover(album.id, 384));
		link.append(art, el('span', 'title ellipsis', album.title), el('span', 'artist ellipsis', album.artist ?? ''));
		if (album.reason) link.append(el('span', 'reason', album.reason));
		return link;
	}

	/** "48 min", "1 Std 12 min", as the server-rendered pages write a length; '' if unknown. */
	function humanDuration(seconds) {
		if (!(seconds > 0)) return '';
		if (seconds < 60) return '<1 min';
		const hours = Math.floor(seconds / 3600);
		const minutes = Math.round((seconds % 3600) / 60);
		if (!hours) return `${minutes} min`;
		return minutes ? `${hours} Std ${minutes} min` : `${hours} Std`;
	}

	function icon(name) {
		const NS = 'http://www.w3.org/2000/svg';
		const svg = document.createElementNS(NS, 'svg');
		svg.setAttribute('class', 'icon');
		svg.setAttribute('viewBox', '0 0 24 24');
		svg.setAttribute('aria-hidden', 'true');
		svg.setAttribute('focusable', 'false');
		const use = document.createElementNS(NS, 'use');
		use.setAttribute('href', `#icon-${name}`);
		svg.append(use);
		return svg;
	}

	/** A thin bar for something begun; nothing for something untouched. */
	function progressBar(position, duration) {
		if (!(position > 0) || !(duration > 0)) return null;
		const bar = el('span', 'bar');
		const fill = el('span', 'fill');
		fill.style.width = `${Math.min(100, Math.round((position / duration) * 100))}%`;
		bar.append(fill);
		bar.setAttribute('aria-hidden', 'true');
		return bar;
	}

	/** A book in a rail. `detail` picks the line under the author, per row. */
	function bookTile(book, detail) {
		const link = el('a', 'discover-album');
		link.href = `/library/books/${book.id}`;
		const art = el('div', 'art');
		if (book.has_cover) art.append(cover(book.id, 384));
		const bar = progressBar(book.position, book.duration);
		if (bar) art.append(bar);
		link.append(art, el('span', 'title ellipsis', book.title), el('span', 'artist ellipsis', book.author ?? ''));
		const line = detail(book);
		if (line) link.append(el('span', 'reason', line));
		return link;
	}

	function volumeOf(book) {
		if (!book.series) return '';
		return book.series_index === null ? book.series : `${book.series} · Band ${String(book.series_index).replace('.', ',')}`;
	}

	function leftOf(book) {
		const left = humanDuration(book.duration - book.position);
		return left ? `noch ${left}` : '';
	}

	/**
	 * An episode with its podcast and a play button. The player (static/js/player) finds the row
	 * through `data-episode` and downloads the episode first if `data-downloaded` says it must.
	 */
	function episodeRow(episode) {
		const row = el('li', 'discover-episode');
		row.dataset.episode = `${episode.id}`;
		row.dataset.downloaded = `${episode.downloaded}`;
		const art = el('a', 'art');
		art.href = `/library/podcasts/${episode.podcast_id}`;
		art.setAttribute('aria-label', episode.podcast_title);
		if (episode.has_cover) art.append(cover(episode.id, 256));
		const body = el('div', 'body');
		const show = el('a', 'show ellipsis', episode.podcast_title);
		show.href = art.href;
		const facts = [];
		if (episode.published_at) facts.push(new Date(episode.published_at).toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric' }));
		const length = episode.position > 0 ? leftOf(episode) : humanDuration(episode.duration);
		if (length) facts.push(length);
		body.append(el('span', 'title', episode.title), show, el('span', 'meta', facts.join(' · ')));
		const bar = progressBar(episode.position, episode.duration);
		if (bar) body.append(bar);
		const play = el('button', 'play');
		play.type = 'button';
		play.dataset.action = 'play-episode';
		play.dataset.episodeId = `${episode.id}`;
		play.title = episode.position > 0 ? 'Weiterhören' : 'Abspielen';
		play.setAttribute('aria-label', `${episode.title} ${episode.position > 0 ? 'weiterhören' : 'abspielen'}`);
		play.append(icon('play-filled'));
		row.append(art, body, play);
		return row;
	}

	function renderHero(root, feed) {
		const target = root.querySelector('[data-feed-hero]');
		if (!target) return;
		const album = feed.suggestions[0];
		if (!album) {
			if (feed.refreshing) target.replaceChildren(el('div', 'discover-skeleton', 'Deine Sammlung wird kennengelernt …'));
			else target.replaceChildren();
			return;
		}
		const card = el('a', 'discover-feature');
		card.href = `/library/albums/${album.id}`;
		const art = el('div', 'art');
		if (album.has_cover) {
			const blur = el('div', 'blur');
			blur.style.backgroundImage = `url("/items/${album.id}/cover?size=384")`;
			card.append(blur);
			art.append(cover(album.id, 512));
		}
		const text = el('div', 'text');
		text.append(el('div', 'kicker', 'FÜR DICH AUSGESUCHT'), el('div', 'title', album.title), el('div', 'artist', album.artist ?? ''), el('p', 'reason', album.reason), playButton('play', 'Abspielen'));
		card.dataset.itemId = `${album.id}`;
		card.append(art, text);
		target.replaceChildren(card);
	}

	function mixCard(mix, index) {
		const card = el('article', 'discover-mix');
		card.dataset.playlistId = `discover-mix-${index}`;
		const top = el('div', 'top');
		const collage = el('div', 'collage');
		if (mix.cover_ids.length) {
			for (let i = 0; i < 4; i += 1) collage.append(cover(mix.cover_ids[i % mix.cover_ids.length], 256));
		}
		const text = el('div');
		text.append(el('h3', '', mix.name), el('p', '', mix.description), el('span', 'meta', `${mix.tracks.length} Titel`));
		top.append(collage, text);
		const details = el('details');
		details.append(el('summary', '', 'Titel anzeigen'), trackList(mix.tracks));
		const actions = el('div', 'actions');
		const buttons = el('div', 'buttons');
		buttons.append(playButton('play-playlist', 'Abspielen'), savePlaylistButton(mix.name, mix.tracks, 'secondary'));
		actions.append(details, buttons);
		card.append(top, actions);
		return card;
	}

	function artistCard(artist) {
		const card = el('article', 'discover-card');
		card.append(el('div', 'name', artist.name), el('p', '', artist.reason));
		if (artist.albums.length) {
			const starters = el('div', 'starters');
			starters.append(...artist.albums.map((album) => el('span', '', album)));
			card.append(starters);
		}
		return card;
	}

	function wantedCard(album) {
		const card = el('article', 'discover-card');
		const name = el('div', 'name');
		name.append(el('span', '', album.title), el('span', 'count', album.year ? String(album.year) : ''));
		card.append(name, el('div', 'by', album.artist), el('p', '', album.reason));
		return card;
	}

	function fill(root, section, selector, nodes) {
		const target = root.querySelector(selector);
		if (!target) return;
		target.replaceChildren(...nodes);
		const wrapper = root.querySelector(`[data-section="${section}"]`);
		if (wrapper) wrapper.hidden = nodes.length === 0;
	}

	function renderFeed(root, feed) {
		renderHero(root, feed);
		fill(root, 'mixes', '[data-feed-mixes]', feed.mixes.map(mixCard));
		// Stays visible while empty, because its button is the way to get mixes back.
		const mixes = root.querySelector('[data-section="mixes"]');
		if (mixes) mixes.hidden = !feed.has_history;
		const noMixes = root.querySelector('[data-feed-mixes-empty]');
		if (noMixes) {
			noMixes.hidden = feed.mixes.length > 0;
			noMixes.querySelector('[data-mixes-title]').textContent = feed.refreshing ? 'Deine Mixes entstehen gerade' : 'Noch keine Mixes';
			noMixes.querySelector('[data-mixes-detail]').textContent = feed.refreshing ? 'Das kann einen Moment dauern.' : 'Aus dem, was du hörst, entstehen hier deine Mixes.';
			noMixes.querySelector('[data-feed-refresh]').hidden = feed.refreshing;
		}
		const again = root.querySelector('[data-mixes-again]');
		if (again) again.hidden = feed.mixes.length === 0;
		fill(root, 'picks', '[data-feed-picks]', feed.suggestions.slice(1).map(albumTile));
		fill(root, 'fresh', '[data-feed-fresh]', feed.fresh.map(albumTile));
		fill(root, 'rediscover', '[data-feed-rediscover]', feed.rediscover.map(albumTile));
		// `?? []`: a page left open across a server update may still talk to the older server.
		fill(root, 'episodes', '[data-feed-episodes]', (feed.new_episodes ?? []).map(episodeRow));
		fill(root, 'series', '[data-feed-series]', (feed.continue_series ?? []).map((book) => bookTile(book, volumeOf)));
		fill(root, 'unstarted', '[data-feed-unstarted]', (feed.unstarted_books ?? []).map((book) => bookTile(book, (b) => humanDuration(b.duration))));
		fill(root, 'paused', '[data-feed-paused]', (feed.rediscover_books ?? []).map((book) => bookTile(book, leftOf)));
		fill(root, 'artists', '[data-feed-artists]', feed.artists.map(artistCard));
		fill(root, 'wanted', '[data-feed-wanted]', feed.wanted.map(wantedCard));
		const hint = root.querySelector('[data-feed-hint]');
		if (hint) hint.textContent = feed.has_history ? '' : 'Hör ein paar Alben an, dann entstehen hier deine Vorschläge.';
		const status = root.querySelector('[data-feed-status]');
		if (status) {
			if (feed.refreshing) status.dataset.busy = '';
			else delete status.dataset.busy;
			status.textContent = feed.refreshing
				? 'Neue Vorschläge werden vorbereitet …'
				: feed.generated_at ? `Aktualisiert: ${new Date(feed.generated_at).toLocaleString('de-DE', { dateStyle: 'short', timeStyle: 'short' })}` : '';
		}
		root.querySelectorAll('[data-feed-refresh]').forEach((button) => {
			button.disabled = feed.refreshing;
		});
	}

	/** The server prepares everything; this only asks again while it is working, and hourly. */
	async function loadFeed(root) {
		clearTimeout(feedTimer);
		try {
			const feed = await call('/ai/feed');
			if (!root.isConnected) return;
			renderFeed(root, feed);
			feedTimer = setTimeout(() => loadFeed(root), feed.refreshing ? 5000 : 3600000);
		} catch (e) {
			const status = root.querySelector('[data-feed-status]');
			if (status) status.textContent = e.message;
		}
	}

	/**
	 * "On Repeat" and "Repeat Rewind": counted from the listening history, so they need no model
	 * and are asked for apart from the feed. A row that cannot be loaded simply stays away.
	 */
	async function loadRepeats(root) {
		const rows = [
			['on-repeat', '/me/on-repeat?limit=20'],
			['repeat-rewind', '/me/repeat-rewind?limit=20']
		];
		await Promise.all(
			rows.map(async ([section, path]) => {
				try {
					const { tracks } = await call(path);
					if (!root.isConnected) return;
					const list = trackList(tracks);
					list.classList.add('columns');
					fill(root, section, `[data-feed-${section}]`, tracks.length ? [list] : []);
				} catch {
					// An older server, or offline: the section stays hidden.
				}
			})
		);
	}

	function bind(root) {
		if (root.dataset.bound) return;
		root.dataset.bound = '1';
		loadFeed(root);
		void loadRepeats(root);
		root.querySelector('[data-ai-playlist-form]')?.addEventListener('submit', (event) => {
			event.preventDefault();
			const form = event.currentTarget;
			const status = root.querySelector('[data-ai-playlist-status]');
			run(form.querySelector('button'), status, async () => {
				const result = await post('/ai/playlist', { prompt: form.elements.prompt.value });
				renderPlaylist(root, result);
				return result.truncated ? CUT.trim() : '';
			});
		});
		root.querySelectorAll('[data-ai-example]').forEach((example) => {
			example.addEventListener('click', () => {
				const input = root.querySelector('[data-ai-playlist-form] input');
				input.value = example.textContent;
				input.focus();
			});
		});
		root.querySelectorAll('[data-feed-refresh]').forEach((button) => {
			button.addEventListener('click', async () => {
				button.disabled = true;
				try {
					await post('/ai/feed/refresh');
					await loadFeed(root);
				} catch (e) {
					button.disabled = false;
					root.querySelector('[data-feed-status]').textContent = e.message;
				}
			});
		});
	}

	function init() {
		const root = page();
		if (root) bind(root);
	}

	document.addEventListener('htmx:load', init);
	document.addEventListener('DOMContentLoaded', init);
	init();
})();
