<script lang="ts">
	let {
		playing = false,
		getAnalyser,
		bars = 5,
		label = 'Wiedergabe läuft'
	}: {
		playing?: boolean;
		getAnalyser?: () => AnalyserNode | null;
		bars?: number;
		label?: string;
	} = $props();

	const FLOOR = 8;
	const CEILING = 100;
	// Hearing is logarithmic and so is music: a linear bin split hands the first columns the
	// bass, where every track sits pinned at the ceiling, and starves the rest. Bands are
	// log-spaced instead, tilted to offset the spectral rolloff towards the highs.
	const MIN_HZ = 45;
	const MAX_HZ = 16000;
	const TILT_PER_OCTAVE = 0.09;
	// Byte levels sit in the top half of their range for almost any real track; the exponent
	// pulls them apart again so the bars show shape instead of one solid block.
	const CONTRAST = 2;
	// Auto-gain: the loudest column reaches the top, quiet passages still show movement.
	const QUIET_REFERENCE = 0.45;
	const PEAK_DECAY = 0.97;
	const FALL = 0.82;

	const columns = $derived(Array.from({ length: bars }, (_, index) => index));

	let root = $state<HTMLElement | null>(null);
	let live = $state(false);

	$effect(() => {
		const element = root;
		const count = bars;
		if (!element || !playing || !getAnalyser) return;

		const node = getAnalyser();
		if (!node) return;
		live = true;

		const spectrum = new Uint8Array(node.frequencyBinCount);
		const nyquist = (node.context?.sampleRate ?? 44100) / 2;
		const top = Math.min(MAX_HZ, nyquist);
		const span = top / MIN_HZ;
		const starts = new Int32Array(count);
		const ends = new Int32Array(count);
		const tilt = new Float32Array(count);
		let cursor = 0;
		for (let index = 0; index < count; index += 1) {
			const low = MIN_HZ * span ** (index / count);
			const high = MIN_HZ * span ** ((index + 1) / count);
			// Bands narrower than a bin would otherwise repeat one another; walking a cursor
			// gives every column its own bin down at the bass end.
			const from = Math.min(
				spectrum.length - 1,
				Math.max(cursor, Math.round((low / nyquist) * spectrum.length))
			);
			const to = Math.min(
				spectrum.length,
				Math.max(from + 1, Math.round((high / nyquist) * spectrum.length))
			);
			starts[index] = from;
			ends[index] = to;
			cursor = to;
			tilt[index] = 1 + TILT_PER_OCTAVE * Math.log2(Math.sqrt(low * high) / MIN_HZ);
		}

		const columnEls = Array.from(element.querySelectorAll<HTMLElement>('.bar'));
		const levels = new Float32Array(count);
		const shown = new Float32Array(count);
		let peak = QUIET_REFERENCE;
		let frame = 0;

		const draw = () => {
			node.getByteFrequencyData(spectrum);
			let loudest = 0;
			for (let index = 0; index < count; index += 1) {
				let band = 0;
				for (let bin = starts[index]; bin < ends[index]; bin += 1) {
					const value = spectrum[bin] ?? 0;
					if (value > band) band = value;
				}
				const level = Math.min(1, (band / 255) ** CONTRAST * tilt[index]);
				levels[index] = level;
				if (level > loudest) loudest = level;
			}
			peak = loudest > peak ? loudest : peak * PEAK_DECAY + loudest * (1 - PEAK_DECAY);
			const gain = 1 / Math.max(QUIET_REFERENCE, peak);
			columnEls.forEach((bar, index) => {
				const target = Math.min(1, levels[index] * gain);
				// Snap up on a transient, sink back slowly, the way a real meter behaves.
				shown[index] = target > shown[index] ? target : shown[index] * FALL + target * (1 - FALL);
				bar.style.height = `${FLOOR + shown[index] * (CEILING - FLOOR)}%`;
			});
			frame = requestAnimationFrame(draw);
		};
		frame = requestAnimationFrame(draw);
		return () => {
			cancelAnimationFrame(frame);
			// Hand the bars back to CSS, or they keep the last frame's heights and sit there
			// as a flat line once playback stops.
			live = false;
			columnEls.forEach((bar) => bar.style.removeProperty('height'));
		};
	});
</script>

<span
	bind:this={root}
	class="viz"
	class:playing
	class:live
	role="img"
	aria-label={playing ? label : 'Pausiert'}
>
	{#each columns as column (column)}
		<span class="bar" style="--i: {column}"></span>
	{/each}
</span>

<style>
	.viz {
		display: flex;
		align-items: flex-end;
		gap: var(--viz-gap, 3px);
		width: 100%;
		height: 100%;
	}
	.bar {
		flex: 1;
		min-width: 2px;
		height: 30%;
		border-radius: 2px;
		background-image: repeating-linear-gradient(
			to top,
			var(--a, var(--music)) 0 calc(var(--viz-segment, 10px) - 3px),
			transparent calc(var(--viz-segment, 10px) - 3px) var(--viz-segment, 10px)
		);
		animation: bounce 900ms ease-in-out infinite alternate;
		animation-delay: calc(var(--i) * -170ms);
		animation-play-state: paused;
		opacity: 0.4;
	}
	.viz.playing .bar {
		animation-play-state: running;
		opacity: 1;
	}
	.viz.live .bar {
		animation: none;
		opacity: 1;
		transition: height 60ms linear;
	}
	@keyframes bounce {
		from {
			height: 18%;
		}
		to {
			height: 100%;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.bar {
			animation: none;
			height: 55%;
		}
		.viz.live .bar {
			transition: none;
		}
	}
</style>
