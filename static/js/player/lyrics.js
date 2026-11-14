// The lyrics tab of the fullscreen player. The text comes from `GET /tracks/{id}/lyrics` — the
// user's own files, see src/library/lyrics.rs — and is only ever set through textContent: it
// was written by whoever tagged the file.

/**
 * The line that is being sung at `seconds`: the last one whose time has come, -1 before the
 * first. `lines` is in playing order, as the server sends it.
 */
export function activeLineIndex(lines, seconds) {
	let low = 0;
	let high = lines.length - 1;
	let found = -1;
	while (low <= high) {
		const middle = (low + high) >> 1;
		if (lines[middle].t <= seconds) {
			found = middle;
			low = middle + 1;
		} else {
			high = middle - 1;
		}
	}
	return found;
}

/**
 * Lines that share a moment light up together (a line with a translation under it): the range
 * of rows around `index` with the same time.
 */
export function activeRange(lines, index) {
	if (index < 0) return [0, -1];
	let first = index;
	while (first > 0 && lines[first - 1].t === lines[index].t) first -= 1;
	return [first, index];
}

/**
 * @param {object} options
 * @param {typeof fetch} [options.fetch]
 */
export function createLyrics(options = {}) {
	const request = options.fetch ?? ((...args) => fetch(...args));
	/** Track id → the answer, or null where the server has no text after all. */
	const cache = new Map();

	async function load(trackId) {
		if (cache.has(trackId)) return cache.get(trackId);
		let lyrics = null;
		try {
			const res = await request(`/tracks/${trackId}/lyrics`);
			if (res.ok) lyrics = await res.json();
			// 404: the flag was ahead of the files. Anything else may pass; ask again next time.
			else if (res.status !== 404) return null;
		} catch {
			return null;
		}
		cache.set(trackId, lyrics);
		return lyrics;
	}

	function build(panel, lyrics) {
		const nodes = lyrics.lines.map((line, index) => {
			if (!line.text) {
				const gap = document.createElement('span');
				gap.className = 'gap';
				gap.dataset.lyricLine = String(index);
				return gap;
			}
			// A timed line is a button: a click jumps there.
			const node = document.createElement(lyrics.synced ? 'button' : 'p');
			node.className = 'line';
			node.dataset.lyricLine = String(index);
			node.textContent = line.text;
			if (lyrics.synced) {
				node.type = 'button';
				node.dataset.action = 'seek-lyric';
				node.dataset.position = String(line.t);
				node.setAttribute('aria-current', 'false');
			}
			return node;
		});
		panel.dataset.synced = String(lyrics.synced);
		panel.replaceChildren(...nodes);
	}

	/** Leaves the lyrics tab when its track has none, back to the list beside it. */
	function leaveTab(root, tab) {
		if (!tab.classList.contains('active')) return;
		const first = root.querySelector('[data-tab]:not([data-np-lyrics-tab])');
		for (const other of root.querySelectorAll('[data-tab]')) {
			other.classList.toggle('active', other === first);
		}
		for (const panel of root.querySelectorAll('[data-tab-panel]')) {
			panel.hidden = panel.dataset.tabPanel !== first?.dataset.tab;
		}
	}

	/**
	 * Brings the tab in line with the player: shown only while the page's item plays a track
	 * that has lyrics, the text of that track loaded, the line of this moment marked and kept
	 * in view.
	 */
	function render(root, store) {
		const tab = root?.querySelector('[data-np-lyrics-tab]');
		const panel = root?.querySelector('[data-np-lyrics]');
		if (!tab || !panel) return;
		const current = store.current;
		const mine = current && !current.live && current.itemId === Number(root.dataset.itemId);
		const track = mine ? current.tracks[current.trackIndex] : null;
		const known = track ? cache.get(track.id) : undefined;
		// `has_lyrics` is what the last scan saw; a 404 since then overrules it.
		const wanted = track?.has_lyrics && known !== null ? track.id : null;
		tab.hidden = wanted === null;
		if (wanted === null) leaveTab(root, tab);

		const shown = panel.dataset.trackId ?? '';
		if (shown !== String(wanted ?? '')) {
			panel.dataset.trackId = String(wanted ?? '');
			delete panel.dataset.active;
			panel.replaceChildren();
			if (wanted !== null) {
				void load(wanted).then((lyrics) => {
					// Another track may have started while this one loaded.
					if (panel.dataset.trackId !== String(wanted) || !panel.isConnected) return;
					if (lyrics) build(panel, lyrics);
					render(root, store);
				});
			}
			return;
		}
		if (!known?.synced || !panel.firstChild) return;

		const index = activeLineIndex(known.lines, store.trackOffset());
		if (panel.dataset.active === String(index)) return;
		panel.dataset.active = String(index);
		const [first, last] = activeRange(known.lines, index);
		for (const node of panel.children) {
			const at = Number(node.dataset.lyricLine);
			if (node.hasAttribute('aria-current')) {
				node.setAttribute('aria-current', String(at >= first && at <= last));
			}
		}
		// Only while the tab is open: scrolling a hidden panel would move the track list.
		if (index >= 0 && !panel.closest('[data-tab-panel]')?.hidden) {
			panel.children[index]?.scrollIntoView?.({ block: 'center', behavior: 'smooth' });
		}
	}

	return { render, load };
}
