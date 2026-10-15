// The seek bars (legacy Scrubber.svelte): `[data-scrubber]` elements, bound once by delegation so
// bars swapped in later work too. The page renders value and max into `data-value`/`data-max`
// through `paintScrubber`; the user's drag is shown until release, then handed to `onSeek`.

const dragging = new Map();

export function isDragging(element) {
	return dragging.has(element);
}

/** Draws `value` of `max` into the bar, unless the user is dragging it. */
export function paintScrubber(element, value, max, format) {
	element.dataset.value = String(value);
	element.dataset.max = String(max);
	if (format) element._format = format;
	if (!dragging.has(element)) draw(element, value);
}

function draw(element, shown) {
	const max = Number(element.dataset.max) || 0;
	const percent = max > 0 ? Math.min(100, Math.max(0, (shown / max) * 100)) : 0;
	const fill = element.querySelector('.fill');
	const thumb = element.querySelector('.thumb');
	if (fill) fill.style.width = `${percent}%`;
	if (thumb) thumb.style.left = `${percent}%`;
	element.setAttribute('aria-valuemax', String(Math.round(max)));
	element.setAttribute('aria-valuenow', String(Math.round(shown)));
	const format = element._format ?? ((s) => `${Math.round(s)} Sekunden`);
	element.setAttribute('aria-valuetext', format(shown));
}

function positionFrom(element, clientX) {
	const max = Number(element.dataset.max) || 0;
	const rect = element.getBoundingClientRect();
	if (rect.width === 0) return 0;
	const ratio = (clientX - rect.left) / rect.width;
	return Math.min(max, Math.max(0, ratio * max));
}

const STEP = 5;
const MOVES = {
	ArrowLeft: -STEP,
	ArrowRight: STEP,
	ArrowDown: -STEP,
	ArrowUp: STEP,
	PageDown: -STEP * 6,
	PageUp: STEP * 6
};

/** Where a key moves the bar: a position, or null for keys it does not handle. */
export function keyTarget(key, value, max) {
	if (max <= 0) return null;
	if (key === 'Home') return 0;
	if (key === 'End') return max;
	const delta = MOVES[key];
	if (delta === undefined) return null;
	return Math.min(max, Math.max(0, value + delta));
}

/** `onSeek(kind, seconds)` with `kind` from `data-scrubber`. */
export function bindScrubbers(onSeek) {
	const scrubberOf = (event) =>
		event.target instanceof Element ? event.target.closest('[data-scrubber]') : null;

	document.addEventListener('pointerdown', (event) => {
		const element = scrubberOf(event);
		if (!element || !(Number(element.dataset.max) > 0)) return;
		element.setPointerCapture?.(event.pointerId);
		element.classList.add('dragging');
		const at = positionFrom(element, event.clientX);
		dragging.set(element, at);
		draw(element, at);
	});
	document.addEventListener('pointermove', (event) => {
		const element = scrubberOf(event);
		if (!element || !dragging.has(element)) return;
		const at = positionFrom(element, event.clientX);
		dragging.set(element, at);
		draw(element, at);
	});
	const release = (event) => {
		const element = scrubberOf(event);
		if (!element || !dragging.has(element)) return;
		const at = positionFrom(element, event.clientX);
		dragging.delete(element);
		element.classList.remove('dragging');
		element.releasePointerCapture?.(event.pointerId);
		draw(element, at);
		onSeek(element.dataset.scrubber, at);
	};
	document.addEventListener('pointerup', release);
	// A cancelled drag (the browser took the gesture over) seeks nowhere.
	document.addEventListener('pointercancel', (event) => {
		const element = scrubberOf(event);
		if (!element || !dragging.has(element)) return;
		dragging.delete(element);
		element.classList.remove('dragging');
		draw(element, Number(element.dataset.value) || 0);
	});
	document.addEventListener('keydown', (event) => {
		const element = scrubberOf(event);
		if (!element) return;
		const at = keyTarget(event.key, Number(element.dataset.value) || 0, Number(element.dataset.max) || 0);
		if (at === null) return;
		event.preventDefault();
		onSeek(element.dataset.scrubber, at);
	});
}
