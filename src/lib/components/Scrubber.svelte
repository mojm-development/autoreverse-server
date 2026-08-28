<script lang="ts">
	let {
		value,
		max,
		onSeek,
		ticks = [],
		label = 'Position',
		format = (seconds: number) => `${Math.round(seconds)} Sekunden`,
		step = 5
	}: {
		value: number;
		max: number;
		onSeek: (seconds: number) => void;
		ticks?: number[];
		label?: string;
		format?: (seconds: number) => string;
		step?: number;
	} = $props();

	let track: HTMLDivElement;
	let dragging = $state<number | null>(null);

	const shown = $derived(dragging ?? value);
	const percent = $derived(max > 0 ? Math.min(100, Math.max(0, (shown / max) * 100)) : 0);

	function positionFrom(clientX: number): number {
		const rect = track.getBoundingClientRect();
		if (rect.width === 0) return 0;
		const ratio = (clientX - rect.left) / rect.width;
		return Math.min(max, Math.max(0, ratio * max));
	}

	function onPointerDown(event: PointerEvent) {
		if (max <= 0) return;
		track.setPointerCapture(event.pointerId);
		dragging = positionFrom(event.clientX);
	}

	function onPointerMove(event: PointerEvent) {
		if (dragging === null) return;
		dragging = positionFrom(event.clientX);
	}

	function onPointerUp(event: PointerEvent) {
		if (dragging === null) return;
		const target = positionFrom(event.clientX);
		dragging = null;
		track.releasePointerCapture(event.pointerId);
		onSeek(target);
	}

	function onKeyDown(event: KeyboardEvent) {
		if (max <= 0) return;
		const moves: Record<string, number> = {
			ArrowLeft: -step,
			ArrowRight: step,
			ArrowDown: -step,
			ArrowUp: step,
			PageDown: -step * 6,
			PageUp: step * 6
		};
		if (event.key === 'Home') {
			event.preventDefault();
			onSeek(0);
			return;
		}
		if (event.key === 'End') {
			event.preventDefault();
			onSeek(max);
			return;
		}
		const delta = moves[event.key];
		if (delta === undefined) return;
		event.preventDefault();
		onSeek(Math.min(max, Math.max(0, value + delta)));
	}
</script>

<div
	bind:this={track}
	class="scrubber"
	class:dragging={dragging !== null}
	role="slider"
	tabindex="0"
	aria-label={label}
	aria-valuemin={0}
	aria-valuemax={Math.round(max)}
	aria-valuenow={Math.round(shown)}
	aria-valuetext={format(shown)}
	onpointerdown={onPointerDown}
	onpointermove={onPointerMove}
	onpointerup={onPointerUp}
	onpointercancel={onPointerUp}
	onkeydown={onKeyDown}
>
	{#each ticks as tick, i (i)}
		<span class="tick" style="left: {max > 0 ? (tick / max) * 100 : 0}%"></span>
	{/each}
	<div class="fill" style="width: {percent}%"></div>
	<div class="thumb" style="left: {percent}%"></div>
</div>

<style>
	.scrubber {
		position: relative;
		width: 100%;
		height: var(--scrubber-height, 4px);
		border-radius: 99px;
		background: var(--track);
		touch-action: none;
		cursor: pointer;
	}
	.scrubber::before {
		content: '';
		position: absolute;
		inset: -8px 0;
	}
	.scrubber:focus-visible {
		outline: 2px solid var(--a, var(--book));
		outline-offset: 4px;
	}
	.fill {
		position: absolute;
		inset: 0 auto 0 0;
		background: var(--a, var(--book));
		border-radius: 99px;
	}
	.tick {
		position: absolute;
		top: -2px;
		width: 1px;
		height: calc(var(--scrubber-height, 4px) + 4px);
		background: var(--track-tick);
	}
	.thumb {
		position: absolute;
		top: 50%;
		width: var(--scrubber-thumb, 10px);
		height: var(--scrubber-thumb, 10px);
		margin-left: calc(var(--scrubber-thumb, 10px) / -2);
		transform: translateY(-50%);
		border-radius: 50%;
		background: var(--text);
	}
</style>
