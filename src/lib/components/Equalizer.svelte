<script lang="ts">
	let { playing = false, label = 'Wiedergabe läuft' }: { playing?: boolean; label?: string } =
		$props();

	const BARS = [0, 1, 2, 3, 4];
</script>

<span class="eq" class:playing role="img" aria-label={playing ? label : 'Pausiert'}>
	{#each BARS as bar (bar)}
		<span class="bar" style="--i: {bar}"></span>
	{/each}
</span>

<style>
	.eq {
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
	.eq.playing .bar {
		animation-play-state: running;
		opacity: 1;
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
	}
</style>
