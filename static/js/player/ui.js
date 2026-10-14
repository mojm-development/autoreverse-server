// Draws the player state into the page: the mini bar (legacy MiniPlayerBar.svelte), the
// fullscreen player (routes/(fullscreen)/library/[id]/player) and the live parts of the book and
// album pages. Everything is looked up fresh on each render, so pages swapped in by a boosted
// navigation need no extra wiring.

import { chapterIndexAt, totalDuration } from './store.js';
import { formatHMS, formatHours, formatSpeed, formatTime } from './format.js';
import { paintScrubber } from './scrubber.js';

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
	if (kind === 'album') return 'music';
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

export function renderMiniPlayer(store, onFullscreenPage) {
	const bar = document.querySelector('[data-mini-player]');
	if (!bar) return;
	const current = store.current;
	const track = current?.tracks[current.trackIndex];
	const show = Boolean(current && track && !onFullscreenPage);
	bar.hidden = !show;
	if (!show) return;

	const byTrack = current.kind === 'album';
	const offset = store.trackOffset();
	const elapsed = byTrack ? offset : current.position;
	const total = byTrack ? track.duration : totalDuration(current.tracks);
	const { skipBack, skipForward } = store.preferences;
	bar.style.setProperty('--a', `var(--${accentFor(current.kind)})`);

	for (const button of bar.querySelectorAll('[data-by-track]')) button.hidden = !byTrack;
	for (const button of bar.querySelectorAll('[data-by-time]')) button.hidden = byTrack;
	const previous = bar.querySelector('[data-action="previous"]');
	const next = bar.querySelector('[data-action="next"]');
	if (previous) previous.disabled = current.trackIndex === 0 && offset <= 3;
	if (next) next.disabled = current.trackIndex >= current.tracks.length - 1;
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
		const src = `/items/${current.itemId}/cover`;
		cover.hidden = !current.hasCover;
		if (current.hasCover && cover.getAttribute('src') !== src) cover.setAttribute('src', src);
	}
	setText(bar.querySelector('[data-mini-title]'), track.title ?? '');
	setText(bar.querySelector('[data-mini-time]'), `${formatTime(elapsed)} / ${formatTime(total)}`);
	const scrubber = bar.querySelector('[data-scrubber]');
	if (scrubber) {
		scrubber.setAttribute('aria-label', byTrack ? 'Position im Titel' : 'Position');
		paintScrubber(scrubber, elapsed, total, formatTime);
	}
	setText(bar.querySelector('[data-mini-speed]'), formatSpeed(current.speed));
	const expand = bar.querySelector('[data-mini-expand]');
	const playerHref = `/library/${current.itemId}/player`;
	if (expand && expand.getAttribute('href') !== playerHref) {
		expand.setAttribute('href', playerHref);
		// htmx reads a boosted link's href once, when it processes the element; without processing it
		// again the link keeps requesting the href it had at page load (/library).
		globalThis.htmx?.process(expand);
	}
}

export function renderFullscreen(store, root) {
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
	if (previous) {
		previous.disabled = byTrack
			? isPlayingThis && current.trackIndex === 0 && offset <= 3
			: chapterIndex <= 0;
	}
	if (next) {
		next.disabled = byTrack
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

export function render(store) {
	const fullscreen = document.querySelector('[data-np]');
	renderMiniPlayer(store, Boolean(fullscreen));
	if (fullscreen) renderFullscreen(store, fullscreen);
	for (const root of document.querySelectorAll('[data-live-item]')) renderLiveItem(store, root);
}

