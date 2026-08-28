<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { page } from '$app/state';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from './Icon.svelte';

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let audioEl: HTMLAudioElement;
	onMount(() => {
		player.attachAudioElement(audioEl);
	});
	const isFullscreenPlayer = $derived(page.url.pathname.endsWith('/player'));
	const current = $derived(player.current);
	const track = $derived(current?.tracks[current.trackIndex]);
	const byTrack = $derived(current?.kind === 'album');
	const elapsed = $derived(byTrack ? player.trackOffset() : (current?.position ?? 0));
	const total = $derived(
		byTrack
			? (track?.duration ?? 0)
			: (current?.tracks.reduce((sum, t) => sum + t.duration, 0) ?? 0)
	);
	const percent = $derived(total > 0 ? (elapsed / total) * 100 : 0);
	const atFirstTrack = $derived(current?.trackIndex === 0);
	const atLastTrack = $derived(
		current !== null && current !== undefined && current.trackIndex >= current.tracks.length - 1
	);

	function formatTime(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
	}
</script>

{#if current && track && !isFullscreenPlayer}
	<div class="bar">
		<div class="transport">
			{#if byTrack}
				<button
					class="icon-btn"
					aria-label="Vorheriger Titel"
					disabled={atFirstTrack && player.trackOffset() <= 3}
					onclick={() => player.previousTrack()}
				>
					<Icon name="previous" />
				</button>
			{:else}
				<button
					class="icon-btn skip"
					aria-label="30 Sekunden zurück"
					onclick={() => player.skipBack(30)}
				>
					30
				</button>
			{/if}
			<button
				class="play"
				aria-label={current.playing ? 'Pause' : 'Abspielen'}
				onclick={() => (current.playing ? player.pause() : player.resume())}
			>
				<Icon name={current.playing ? 'pause' : 'play'} />
			</button>
			{#if byTrack}
				<button
					class="icon-btn"
					aria-label="Nächster Titel"
					disabled={atLastTrack}
					onclick={() => player.nextTrack()}
				>
					<Icon name="next" />
				</button>
			{:else}
				<button
					class="icon-btn skip"
					aria-label="15 Sekunden vor"
					onclick={() => player.skipForward(15)}
				>
					15
				</button>
			{/if}
		</div>
		<div class="cover"></div>
		<div class="info">
			<div class="line">
				<span class="title">{track.title}</span>
				<span class="time mono">{formatTime(elapsed)} / {formatTime(total)}</span>
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
		position: fixed;
		left: var(--sidebar-width);
		right: 0;
		bottom: 0;
		z-index: 20;
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
	.icon-btn:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.skip {
		font: 600 10.5px var(--font-mono);
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

	@media (max-width: 700px) {
		.bar {
			left: 0;
			bottom: var(--mobile-nav-height);
		}
	}
</style>
