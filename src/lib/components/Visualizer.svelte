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
	const USABLE_SPECTRUM = 0.62;

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
		const usable = Math.max(count, Math.floor(spectrum.length * USABLE_SPECTRUM));
		const width = Math.max(1, Math.floor(usable / count));
		const columnEls = Array.from(element.querySelectorAll<HTMLElement>('.bar'));
		let frame = 0;

		const draw = () => {
			node.getByteFrequencyData(spectrum);
			columnEls.forEach((bar, index) => {
				let sum = 0;
				for (let offset = 0; offset < width; offset += 1) {
					sum += spectrum[index * width + offset] ?? 0;
				}
				const level = sum / width / 255;
				bar.style.height = `${Math.min(CEILING, FLOOR + level * 135)}%`;
			});
			frame = requestAnimationFrame(draw);
		};
		frame = requestAnimationFrame(draw);
		return () => cancelAnimationFrame(frame);
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
		transition: height 80ms linear;
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
