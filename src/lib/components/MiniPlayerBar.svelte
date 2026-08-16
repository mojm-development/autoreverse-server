<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from './Icon.svelte';

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let audioEl: HTMLAudioElement;
	onMount(() => {
		player.attachAudioElement(audioEl);
	});
	const current = $derived(player.current);
	const track = $derived(current?.tracks[current.trackIndex]);
	const totalDuration = $derived(current?.tracks.reduce((sum, t) => sum + t.duration, 0) ?? 0);
	const percent = $derived(
		totalDuration > 0 ? ((current?.position ?? 0) / totalDuration) * 100 : 0
	);

	function formatTime(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
	}
</script>

{#if current && track}
	<div class="bar">
		<div class="transport">
			<button aria-label="Zurück" onclick={() => player.skipBack(30)}
				><Icon name="previous" /></button
			>
			<button
				class="play"
				aria-label={current.playing ? 'Pause' : 'Abspielen'}
				onclick={() => (current.playing ? player.pause() : player.resume())}
			>
				<Icon name={current.playing ? 'pause' : 'play'} />
			</button>
			<button aria-label="Vor" onclick={() => player.skipForward(15)}><Icon name="next" /></button>
		</div>
		<div class="cover"></div>
		<div class="info">
			<div class="line">
				<span class="title">{track.title}</span>
				<span class="time mono">{formatTime(current.position)} / {formatTime(totalDuration)}</span>
			</div>
			<div class="scrubber">
				<div class="fill" style="width: {percent}%"></div>
				<div class="thumb" style="left: {percent}%"></div>
			</div>
		</div>
		<div class="extra">
			<span class="pill mono"
				>{current.speed.toLocaleString('de-DE', { minimumFractionDigits: 2 })}×</span
			>
		</div>
	</div>
{/if}

<audio bind:this={audioEl} hidden></audio>

<style>
	.bar {
		/* grid-column: 2 takes effect once a later task's /library shell wires
		   this bar into an actual `display: grid` layout with a sidebar in
		   column 1 (Task 30+). At the root layout level (this task's scope)
		   there is no grid ancestor, so this is currently inert — not
		   dead/accidental CSS, just not yet load-bearing. */
		grid-column: 2;
		display: flex;
		align-items: center;
		gap: 16px;
		padding: 0 24px;
		border-top: 1px solid var(--line);
		background: var(--sidebar);
		height: 78px;
	}
	.transport {
		display: flex;
		align-items: center;
		gap: 14px;
	}
	.play {
		width: 34px;
		height: 34px;
		border-radius: 50%;
		background: var(--a, var(--book));
		display: grid;
		place-items: center;
		border: none;
	}
	.cover {
		width: 42px;
		height: 42px;
		flex: none;
		border-radius: 6px;
		background: var(--tile);
	}
	.info {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 5px;
		min-width: 0;
	}
	.line {
		display: flex;
		align-items: baseline;
		gap: 8px;
	}
	.title {
		font: 500 12.5px/1.2 var(--font-sans);
		white-space: nowrap;
	}
	.time {
		margin-left: auto;
		font-size: 10.5px;
		color: var(--faint);
	}
	.scrubber {
		position: relative;
		height: 4px;
		border-radius: 99px;
		background: var(--track);
	}
	.fill {
		position: absolute;
		inset: 0 auto 0 0;
		background: var(--a, var(--book));
		border-radius: 99px;
	}
	.thumb {
		position: absolute;
		top: -3px;
		width: 10px;
		height: 10px;
		border-radius: 50%;
		background: var(--text);
		margin-left: -5px;
	}
</style>
