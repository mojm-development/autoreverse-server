<script lang="ts">
	let {
		playing = false,
		getAnalyser,
		label = 'Wiedergabe läuft'
	}: {
		playing?: boolean;
		getAnalyser?: () => AnalyserNode | null;
		label?: string;
	} = $props();

	const BARS = [0, 1, 2, 3, 4];
	const FLOOR = 14;
	const CEILING = 100;

	let root = $state<HTMLElement | null>(null);
	let live = $state(false);

	$effect(() => {
		const element = root;
		if (!element || !playing || !getAnalyser) return;

		const node = getAnalyser();
		if (!node) return;
		live = true;

		const spectrum = new Uint8Array(node.frequencyBinCount);
		const bars = Array.from(element.querySelectorAll<HTMLElement>('.bar'));
		const width = Math.max(1, Math.floor(spectrum.length / bars.length));
		let frame = 0;

		const draw = () => {
			node.getByteFrequencyData(spectrum);
			bars.forEach((bar, index) => {
				let sum = 0;
				for (let offset = 0; offset < width; offset += 1) {
					sum += spectrum[index * width + offset] ?? 0;
				}
				const level = sum / width / 255;
				bar.style.height = `${Math.min(CEILING, FLOOR + level * 130)}%`;
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
	{#each BARS as bar (bar)}
		<span class="bar" style="--i: {bar}"></span>
	{/each}
</span>

<style>
	.viz {
		display: inline-flex;
		align-items: flex-end;
		gap: 3px;
		height: 26px;
	}
	.bar {
		width: 3px;
		height: 30%;
		border-radius: 2px;
		background: var(--a, var(--music));
		transform-origin: bottom;
		animation: bounce 900ms ease-in-out infinite alternate;
		animation-delay: calc(var(--i) * -170ms);
		animation-play-state: paused;
		opacity: 0.45;
	}
	.viz.playing .bar {
		animation-play-state: running;
		opacity: 1;
	}
	.viz.live .bar {
		animation: none;
		opacity: 1;
		transition: height 90ms linear;
	}
	@keyframes bounce {
		from {
			height: 22%;
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
