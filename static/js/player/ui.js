// Draws the player state into the page: the mini bar (legacy MiniPlayerBar.svelte), the
// fullscreen player (routes/(fullscreen)/library/[id]/player) and the live parts of the book and
// album pages. Everything is looked up fresh on each render, so pages swapped in by a boosted
// navigation need no extra wiring.

import { chapterIndexAt, totalDuration } from './store.js';
import { formatHMS, formatHours, formatSpeed, formatTime } from './format.js';
import { paintScrubber } from './scrubber.js';
import { mediaLine } from './devices.js';

function setIcon(container, name) {
	const svg = container?.querySelector('svg.icon');
	if (!svg) return;
	svg.dataset.testid = `icon-${name}`;
	svg.querySelector('use')?.setAttribute('href', `#icon-${name}`);
}

function setText(element, text) {
	if (element && element.textContent !== text) element.textContent = text;
}

function accentFor(kind) {
	if (kind === 'book') return 'book';
	if (kind === 'album' || kind === 'station') return 'music';
	return 'podcast';
}

/** The chapters a page rendered, read back from its rows. */
function pageChapters(root) {
	return Array.from(root.querySelectorAll('[data-chapter]')).map((row) => ({
		row,
		start: Number(row.dataset.start),
		end: Number(row.dataset.end),
		label: row.dataset.label ?? '',
		title: row.dataset.title ?? ''
	}));
}

/** legacy ChapterList.svelte: "läuft" on the current chapter, "gehört" on the ones before. */
function renderChapterRows(chapters, position, isPlayingThis) {
	const current =
		chapters.find((c) => position >= c.start && position < c.end) ??
		chapters[chapters.length - 1] ??
		null;
	for (const c of chapters) {
		const playing = isPlayingThis && c === current;
		const heard = !playing && position >= c.end;
		c.row.setAttribute('aria-current', String(playing));
		setText(c.row.querySelector('[data-chapter-state]'), playing ? 'läuft' : heard ? 'gehört' : '');
	}
	return current;
}

function renderTrackRows(root, playingTrackId) {
	for (const row of root.querySelectorAll('[data-track-row]')) {
		const playing = playingTrackId !== null && Number(row.dataset.trackId) === playingTrackId;
		row.setAttribute('aria-current', String(playing));
	}
}

/** The playlist page: the playing entry's row is the current one. */
export function renderPlaylistRows(root, playingEntryId) {
	for (const row of root.querySelectorAll('[data-entry-id]')) {
		row.setAttribute('aria-current', String(row.dataset.entryId === playingEntryId));
	}
}

/** What a playing station is called in the mini bar: its name, and the running title if known. */
export function stationLine(current) {
	const name = current.tracks[0]?.title ?? '';
	const now = current.nowPlaying?.raw;
	return now ? `${name} · ${now}` : name;
}

/**
 * The radio page: the tile of the station that plays is the current one and shows the running
 * title; its button pauses instead of starting.
 */
export function renderStations(store) {
	const current = store.current;
	const playingId = current?.live ? current.stationId : null;
	for (const tile of document.querySelectorAll('[data-station-id]')) {
		const mine = playingId !== null && Number(tile.dataset.stationId) === playingId;
		tile.setAttribute('aria-current', String(mine));
		setText(tile.querySelector('[data-station-now]'), mine ? (current.nowPlaying?.raw ?? '') : '');
		const button = tile.querySelector('[data-action="play-station"]');
		if (!button) continue;
		const name = tile.dataset.stationName ?? '';
		const label = mine && current.playing ? `${name} anhalten` : `${name} live hören`;
		if (button.getAttribute('aria-label') !== label) button.setAttribute('aria-label', label);
		button.setAttribute('aria-pressed', String(mine && current.playing));
	}
}

/**
 * The mini bar as the remote of another device (`remote` is `{device, position}`, see
 * devices.js): "Läuft auf <Gerät>", play/pause, previous/next, the position as that device
 * would show it now, its volume where it can be set, and "Hier weiterhören". The buttons are
 * the bar's own; main.js sends their actions to the device while this mode is on.
 */
function renderRemote(bar, { device, position }) {
	const state = device.state;
	const media = state.media;
	const live = media.kind === 'station';
	const accent = media.kind === 'book' ? 'book' : media.kind === 'episode' ? 'podcast' : 'music';
	bar.style.setProperty('--a', `var(--${accent})`);
	if (bar.dataset.live !== String(live)) bar.dataset.live = String(live);
	for (const button of bar.querySelectorAll('[data-by-track]')) {
		button.hidden = live;
		button.disabled = false;
	}
	for (const button of bar.querySelectorAll('[data-by-time]')) button.hidden = true;
	bar.querySelector('[data-action="previous"]')?.setAttribute('aria-label', 'Zurück');
	bar.querySelector('[data-action="next"]')?.setAttribute('aria-label', 'Weiter');
	const play = bar.querySelector('[data-action="toggle"]');
	play?.setAttribute('aria-label', state.playing ? 'Pause' : 'Abspielen');
	setIcon(play, state.playing ? 'pause' : 'play');
	const cover = bar.querySelector('[data-mini-cover]');
	if (cover) cover.hidden = true;
	setText(bar.querySelector('[data-mini-title]'), `Läuft auf ${device.name} · ${mediaLine(media)}`);
	const seekable = !live && state.duration > 0;
	setText(
		bar.querySelector('[data-mini-time]'),
		live ? 'Live' : seekable ? `${formatTime(position)} / ${formatTime(state.duration)}` : formatTime(position)
	);
	const scrubber = bar.querySelector('[data-scrubber]');
	if (scrubber) {
		scrubber.hidden = !seekable;
		scrubber.setAttribute('aria-label', `Position auf ${device.name}`);
		if (seekable) paintScrubber(scrubber, position, state.duration, formatTime);
	}
	const speed = bar.querySelector('[data-mini-speed]');
	if (speed) speed.hidden = true;
	const expand = bar.querySelector('[data-mini-expand]');
	if (expand) expand.hidden = true;
	const volume = bar.querySelector('[data-remote-volume]');
	if (volume) {
		// `null`: that device's volume cannot be set from afar.
		volume.hidden = typeof state.volume !== 'number';
		if (!volume.hidden && document.activeElement !== volume) volume.value = String(state.volume);
	}
}

/**
 * `queue` is `{previous, next}` while a playlist plays (static/js/player/queue.js): then the
 * mini bar shows previous/next for every kind, beside the skips of a book or episode.
 * `remote` is another device's playback to show while nothing plays here, or null.
 */
export function renderMiniPlayer(store, onFullscreenPage, queue = null, remote = null) {
	const bar = document.querySelector('[data-mini-player]');
	if (!bar) return;
	const current = store.current;
	const track = current?.tracks[current.trackIndex];
	const show = Boolean(current && track && !onFullscreenPage);
	const remoteShown = Boolean(!current && remote && !onFullscreenPage);
	bar.hidden = !show && !remoteShown;
	if (bar.dataset.remote !== String(remoteShown)) bar.dataset.remote = String(remoteShown);
	// "Hier weiterhören": bring the other device's playback here — or start what was handed
	// over, where the browser would not play it without a click.
	const here = bar.querySelector('[data-mini-here]');
	if (here) here.hidden = !remoteShown && !(show && current.refused);
	if (remoteShown) {
		renderRemote(bar, remote);
		return;
	}
	const remoteVolume = bar.querySelector('[data-remote-volume]');
	if (remoteVolume) remoteVolume.hidden = true;
	if (!show) return;

	// A radio station: play/pause only — no tracks to step through, no time to skip or scrub.
	const live = Boolean(current.live);
	const byTrack = current.kind === 'album';
	const offset = store.trackOffset();
	const elapsed = byTrack ? offset : current.position;
	const total = byTrack ? track.duration : totalDuration(current.tracks);
	const { skipBack, skipForward } = store.preferences;
	bar.style.setProperty('--a', `var(--${accentFor(current.kind)})`);
	if (bar.dataset.live !== String(live)) bar.dataset.live = String(live);

	for (const button of bar.querySelectorAll('[data-by-track]')) {
		button.hidden = live || (!byTrack && !queue);
	}
	for (const button of bar.querySelectorAll('[data-by-time]')) button.hidden = live || byTrack;
	const previous = bar.querySelector('[data-action="previous"]');
	const next = bar.querySelector('[data-action="next"]');
	if (previous) {
		previous.disabled = queue ? !queue.previous : current.trackIndex === 0 && offset <= 3;
		const label = queue ? 'Vorheriger Eintrag' : 'Vorheriger Titel';
		if (previous.getAttribute('aria-label') !== label) previous.setAttribute('aria-label', label);
	}
	if (next) {
		next.disabled = queue ? !queue.next : current.trackIndex >= current.tracks.length - 1;
		const label = queue ? 'Nächster Eintrag' : 'Nächster Titel';
		if (next.getAttribute('aria-label') !== label) next.setAttribute('aria-label', label);
	}
	const back = bar.querySelector('[data-action="skip-back"]');
	const forward = bar.querySelector('[data-action="skip-forward"]');
	if (back) {
		back.setAttribute('aria-label', `${skipBack} Sekunden zurück`);
		setText(back, String(skipBack));
	}
	if (forward) {
		forward.setAttribute('aria-label', `${skipForward} Sekunden vor`);
		setText(forward, String(skipForward));
	}

	const play = bar.querySelector('[data-action="toggle"]');
	play?.setAttribute('aria-label', current.playing ? 'Pause' : 'Abspielen');
	setIcon(play, current.playing ? 'pause' : 'play');

	const cover = bar.querySelector('[data-mini-cover]');
	if (cover) {
		const src = live
			? `/radio/stations/${current.stationId}/logo?size=128`
			: `/items/${current.itemId}/cover?size=128`;
		const has = live ? current.hasLogo : current.hasCover;
		cover.hidden = !has;
		if (has && cover.getAttribute('src') !== src) cover.setAttribute('src', src);
	}
	setText(bar.querySelector('[data-mini-title]'), live ? stationLine(current) : (track.title ?? ''));
	setText(
		bar.querySelector('[data-mini-time]'),
		live ? 'Live' : `${formatTime(elapsed)} / ${formatTime(total)}`
	);
	const scrubber = bar.querySelector('[data-scrubber]');
	if (scrubber) {
		scrubber.hidden = live;
		scrubber.setAttribute('aria-label', byTrack ? 'Position im Titel' : 'Position');
		if (!live) paintScrubber(scrubber, elapsed, total, formatTime);
	}
	const speed = bar.querySelector('[data-mini-speed]');
	if (speed) speed.hidden = live;
	setText(speed, formatSpeed(current.speed));
	const expand = bar.querySelector('[data-mini-expand]');
	// A station has no fullscreen player to open.
	if (expand) expand.hidden = live;
	const playerHref = `/library/${current.itemId}/player`;
	if (!live && expand && expand.getAttribute('href') !== playerHref) {
		expand.setAttribute('href', playerHref);
		// htmx reads a boosted link's href once, when it processes the element; without processing it
		// again the link keeps requesting the href it had at page load (/library).
		globalThis.htmx?.process(expand);
	}
}

export function renderFullscreen(store, root, queue = null) {
	const itemId = Number(root.dataset.itemId);
	const kind = root.dataset.kind;
	const itemTitle = root.dataset.title ?? '';
	const current = store.current;
	const isPlayingThis = current?.itemId === itemId;
	const byTrack = kind === 'album';
	const position = isPlayingThis ? current.position : 0;
	const currentTrack = isPlayingThis ? (current.tracks[current.trackIndex] ?? null) : null;
	const offset = isPlayingThis ? store.trackOffset() : 0;
	const elapsed = byTrack ? offset : position;
	const total = byTrack
		? (currentTrack?.duration ?? 0)
		: isPlayingThis
			? totalDuration(current.tracks)
			: 0;

	const chapters = pageChapters(root);
	const chapterIndex = chapterIndexAt(chapters, position);
	const chapter = chapterIndex >= 0 ? chapters[chapterIndex] : null;
	renderChapterRows(chapters, position, isPlayingThis);
	renderTrackRows(root, currentTrack ? currentTrack.id : null);

	// The headline is whatever plays right now; the item's own title moves to the second line.
	const headline = byTrack
		? (currentTrack?.title ?? itemTitle)
		: chapter
			? chapter.label || chapter.title
			: itemTitle;
	setText(root.querySelector('[data-np-headline]'), headline);
	const trackCount = Number(root.dataset.trackCount) || 0;
	let subtitle = '';
	if (byTrack && currentTrack) {
		subtitle = `Titel ${current.trackIndex + 1} von ${trackCount} · ${itemTitle}`;
	} else if (!byTrack && chapter) {
		subtitle = `Kapitel ${chapterIndex + 1} von ${chapters.length} · ${itemTitle}`;
	}
	setText(root.querySelector('[data-np-subtitle]'), subtitle);

	const scrubber = root.querySelector('[data-scrubber]');
	if (scrubber) paintScrubber(scrubber, elapsed, total, formatHMS);
	setText(root.querySelector('[data-np-elapsed]'), formatHMS(elapsed));
	setText(root.querySelector('[data-np-total]'), formatHMS(total));

	const playing = Boolean(current?.playing);
	const play = root.querySelector('[data-action="toggle"]');
	if (play) {
		play.setAttribute('aria-label', playing ? 'Pause' : 'Abspielen');
		play.title = playing ? 'Pause (Leertaste)' : 'Abspielen (Leertaste)';
		setIcon(play, playing ? 'pause-filled' : 'play-filled');
	}
	const previous = root.querySelector('[data-action="jump-previous"]');
	const next = root.querySelector('[data-action="jump-next"]');
	// Inside a playlist the jumps reach the neighbouring entries too.
	const queued = isPlayingThis && queue;
	if (previous) {
		previous.disabled = queued
			? !queue.previous
			: byTrack
				? isPlayingThis && current.trackIndex === 0 && offset <= 3
				: chapterIndex <= 0;
	}
	if (next) {
		next.disabled = queued
			? !queue.next
			: byTrack
				? isPlayingThis && current.trackIndex >= current.tracks.length - 1
				: chapterIndex < 0 || chapterIndex >= chapters.length - 1;
	}

	const stepBack = store.nudgeStep(-1, kind);
	const stepForward = store.nudgeStep(1, kind);
	for (const [selector, step, word, key] of [
		['[data-step="back"]', stepBack, 'zurück', '←'],
		['[data-step="forward"]', stepForward, 'vor', '→']
	]) {
		const button = root.querySelector(selector);
		if (!button) continue;
		button.setAttribute('aria-label', `${step} Sekunden ${word}`);
		button.title = `${step} s ${word} (${key})`;
		setText(button.querySelector('.step-value'), String(step));
	}
	const listLabel = byTrack ? 'Titel' : 'Kapitel';
	setText(
		root.querySelector('[data-np-hints]'),
		`Leertaste Play · ← ${stepBack} s · → ${stepForward} s · Shift+←/→ ${listLabel} · Esc zurück`
	);
	const speed = root.querySelector('[data-np-speed]');
	if (speed && document.activeElement !== speed) {
		speed.value = String(store.preferences.playbackSpeed);
	}
}

/** The book and album detail pages follow the player while it plays their item. */
export function renderLiveItem(store, root) {
	const itemId = Number(root.dataset.itemId);
	const current = store.current;
	if (current?.itemId !== itemId) {
		renderTrackRows(root, null);
		return;
	}
	const track = current.tracks[current.trackIndex];
	renderTrackRows(root, track ? track.id : null);
	if (root.dataset.kind !== 'book') return;

	const position = current.position;
	const chapter = renderChapterRows(pageChapters(root), position, true);
	const total = Number(root.dataset.total) || totalDuration(current.tracks);
	const percent = total > 0 ? Math.round((position / total) * 100) : 0;
	setText(root.querySelector('[data-live-chapter-name]'), chapter?.title ?? '');
	setText(
		root.querySelector('[data-live-meta]'),
		`${formatHours(Math.max(0, total - position))} übrig · ${percent} %`
	);
	const fill = root.querySelector('[data-live-fill]');
	if (fill) fill.style.width = `${Math.min(100, Math.max(0, percent))}%`;
}

export function render(store, queue = null, remote = null) {
	const fullscreen = document.querySelector('[data-np]');
	renderMiniPlayer(store, Boolean(fullscreen), queue, remote);
	if (fullscreen) renderFullscreen(store, fullscreen, queue);
	for (const root of document.querySelectorAll('[data-live-item]')) renderLiveItem(store, root);
	renderStations(store);
}

