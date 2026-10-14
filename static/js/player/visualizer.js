// The spectrum bars (legacy Visualizer.svelte). Every `[data-viz]` on the page draws from the
// player's one analyser in a single animation loop; without an analyser (or paused) the bars fall
// back to the CSS bounce, or stand still.

const FLOOR = 8;
const CEILING = 100;
// Hearing is logarithmic and so is music: bands are log-spaced, tilted to offset the spectral
// rolloff towards the highs.
const MIN_HZ = 45;
const MAX_HZ = 16000;
const TILT_PER_OCTAVE = 0.09;
// Byte levels sit in the top half of their range for almost any real track; the exponent pulls
// them apart again so the bars show shape instead of one solid block.
const CONTRAST = 2;
// Auto-gain: the loudest column reaches the top, quiet passages still show movement.
const QUIET_REFERENCE = 0.45;
const PEAK_DECAY = 0.97;
const FALL = 0.82;

/** Bin ranges and tilt for `count` columns over a spectrum of `bins` bins. */
export function bandLayout(count, bins, sampleRate) {
	const nyquist = sampleRate / 2;
	const top = Math.min(MAX_HZ, nyquist);
	const span = top / MIN_HZ;
	const starts = new Int32Array(count);
	const ends = new Int32Array(count);
	const tilt = new Float32Array(count);
	let cursor = 0;
	for (let index = 0; index < count; index += 1) {
		const low = MIN_HZ * span ** (index / count);
		const high = MIN_HZ * span ** ((index + 1) / count);
		// Bands narrower than a bin would repeat one another; the cursor gives every column its
		// own bin down at the bass end.
		const from = Math.min(bins - 1, Math.max(cursor, Math.round((low / nyquist) * bins)));
		const to = Math.min(bins, Math.max(from + 1, Math.round((high / nyquist) * bins)));
		starts[index] = from;
		ends[index] = to;
		cursor = to;
		tilt[index] = 1 + TILT_PER_OCTAVE * Math.log2(Math.sqrt(low * high) / MIN_HZ);
	}
	return { starts, ends, tilt };
}

const states = new WeakMap();

function stateFor(element, count, bins, sampleRate) {
	let state = states.get(element);
	if (!state || state.count !== count || state.bins !== bins) {
		state = {
			count,
			bins,
			layout: bandLayout(count, bins, sampleRate),
			levels: new Float32Array(count),
			shown: new Float32Array(count),
			peak: QUIET_REFERENCE,
			bars: Array.from(element.querySelectorAll('.bar'))
		};
		states.set(element, state);
	}
	return state;
}

function drawOne(element, spectrum, sampleRate) {
	const bars = element.querySelectorAll('.bar');
	const state = stateFor(element, bars.length, spectrum.length, sampleRate);
	const { starts, ends, tilt } = state.layout;
	let loudest = 0;
	for (let index = 0; index < state.count; index += 1) {
		let band = 0;
		for (let bin = starts[index]; bin < ends[index]; bin += 1) {
			const value = spectrum[bin] ?? 0;
			if (value > band) band = value;
		}
		const level = Math.min(1, (band / 255) ** CONTRAST * tilt[index]);
		state.levels[index] = level;
		if (level > loudest) loudest = level;
	}
	state.peak =
		loudest > state.peak ? loudest : state.peak * PEAK_DECAY + loudest * (1 - PEAK_DECAY);
	const gain = 1 / Math.max(QUIET_REFERENCE, state.peak);
	state.bars.forEach((bar, index) => {
		const target = Math.min(1, state.levels[index] * gain);
		// Snap up on a transient, sink back slowly, the way a real meter behaves.
		state.shown[index] =
			target > state.shown[index] ? target : state.shown[index] * FALL + target * (1 - FALL);
		bar.style.height = `${FLOOR + state.shown[index] * (CEILING - FLOOR)}%`;
	});
}

/** Hands the bars back to CSS, or they keep the last frame's heights as a flat line. */
function release(element) {
	element.classList.remove('live');
	for (const bar of element.querySelectorAll('.bar')) bar.style.removeProperty('height');
	states.delete(element);
}

/**
 * Keeps every `[data-viz]` in step with the player. Call `sync()` whenever the player state or
 * the page changes: it sets the CSS state and runs the spectrum loop while something plays. The
 * loop only asks for an analyser while playing — that is what creates the AudioContext, which
 * needs the user's gesture behind it.
 */
export function createVisualizers({ isPlaying, getAnalyser }) {
	let spectrum = null;
	let running = false;

	function frame() {
		const elements = document.querySelectorAll('[data-viz]');
		const node = isPlaying() ? getAnalyser() : null;
		if (!node) {
			for (const element of elements) if (element.classList.contains('live')) release(element);
			running = false;
			return;
		}
		if (!spectrum || spectrum.length !== node.frequencyBinCount) {
			spectrum = new Uint8Array(node.frequencyBinCount);
		}
		node.getByteFrequencyData(spectrum);
		const sampleRate = node.context?.sampleRate ?? 44100;
		for (const element of elements) {
			// Hidden ones (the mini player behind the fullscreen one) are not drawn.
			if (element.offsetParent === null) continue;
			element.classList.add('live');
			drawOne(element, spectrum, sampleRate);
		}
		requestAnimationFrame(frame);
	}

	function sync() {
		const playing = isPlaying();
		for (const element of document.querySelectorAll('[data-viz]')) {
			element.classList.toggle('playing', playing);
			element.setAttribute(
				'aria-label',
				playing ? (element.dataset.vizLabel ?? 'Wiedergabe läuft') : 'Pausiert'
			);
		}
		if (playing && !running) {
			running = true;
			requestAnimationFrame(frame);
		}
	}

	return { sync };
}
