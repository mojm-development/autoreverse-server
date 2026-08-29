<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from './Icon.svelte';
	import Scrubber from './Scrubber.svelte';
	import Visualizer from './Visualizer.svelte';
	import { bindPlayerShortcuts } from '$lib/playerShortcuts';

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
	// Same accent the library and the fullscreen player use for this kind of item.
	const accent = $derived(
		current?.kind === 'book' ? 'book' : current?.kind === 'album' ? 'music' : 'podcast'
	);
	const atFirstTrack = $derived(current?.trackIndex === 0);
	const atLastTrack = $derived(
		current !== null && current !== undefined && current.trackIndex >= current.tracks.length - 1
	);

	function togglePlay() {
		if (!current) return;
		if (current.playing) player.pause();
		else player.resume();
	}

	// The same keys as the fullscreen player, wherever the mini bar is the visible player.
	// Exactly one binding is live: the fullscreen route hides this bar and binds its own.
	$effect(() => {
		if (isFullscreenPlayer || !current) return;
		return bindPlayerShortcuts((action) => {
			if (action === 'toggle') togglePlay();
			else if (action === 'back') player.nudge(-1);
			else if (action === 'forward') player.nudge(1);
			else if (action === 'previous') player.jumpPrevious();
			else if (action === 'next') player.jumpNext();
		});
	});

	function formatTime(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
	}
</script>

{#if current && track && !isFullscreenPlayer}
	<div class="bar" data-testid="mini-player" style="--a: var(--{accent})">
		<div class="transport">
			{#if byTrack}
				<button
					class="icon-btn"
					aria-label="Vorheriger Titel"
					disabled={atFirstTrack && player.trackOffset() <= 3}
					onclick={() => player.jumpPrevious()}
				>
					<Icon name="previous" />
				</button>
			{:else}
				<button
					class="icon-btn skip"
					aria-label="{player.preferences.skipBack} Sekunden zurück"
					onclick={() => player.skipBack()}
				>
					{player.preferences.skipBack}
				</button>
			{/if}
			<button
				class="play"
				aria-label={current.playing ? 'Pause' : 'Abspielen'}
				onclick={togglePlay}
			>
				<Icon name={current.playing ? 'pause' : 'play'} />
			</button>
			{#if byTrack}
				<button
					class="icon-btn"
					aria-label="Nächster Titel"
					disabled={atLastTrack}
					onclick={() => player.jumpNext()}
				>
					<Icon name="next" />
				</button>
			{:else}
				<button
					class="icon-btn skip"
					aria-label="{player.preferences.skipForward} Sekunden vor"
					onclick={() => player.skipForward()}
				>
					{player.preferences.skipForward}
				</button>
			{/if}
		</div>
		<div class="cover">
			{#if current.hasCover}
				<img src="/items/{current.itemId}/cover" alt="" />
			{/if}
		</div>
		<div class="info">
			<div class="line">
				<span class="title">{track.title}</span>
				<span class="time mono">{formatTime(elapsed)} / {formatTime(total)}</span>
			</div>
			<Scrubber
				value={elapsed}
				max={total}
				label={byTrack ? 'Position im Titel' : 'Position'}
				format={formatTime}
				onSeek={(seconds) => (byTrack ? player.seekInTrack(seconds) : player.seek(seconds))}
			/>
		</div>
		<span class="mini-viz" aria-hidden="true">
			<Visualizer
				bars={14}
				playing={current.playing}
				getAnalyser={player.getAnalyser}
				label="Wiedergabe läuft"
			/>
		</span>
		<div class="extra">
			<span class="pill mono"
				>{current.speed.toLocaleString('de-DE', { minimumFractionDigits: 2 })}×</span
			>
			<a
				class="icon-btn"
				href={resolve('/(fullscreen)/library/[id]/player', { id: String(current.itemId) })}
				aria-label="Großer Player"
			>
				<Icon name="expand" />
			</a>
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
	.extra {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.extra .icon-btn {
		width: 30px;
		height: 30px;
		display: grid;
		place-items: center;
		border-radius: 50%;
		border: 1px solid var(--line);
		color: var(--dim);
	}
	.extra .icon-btn:hover {
		color: var(--text);
		background: var(--panel);
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
	.mini-viz {
		display: block;
		flex: none;
		width: 92px;
		height: 30px;
		--viz-gap: 3px;
		--viz-segment: 6px;
	}
	.cover {
		width: 42px;
		height: 42px;
		flex: none;
		border-radius: 6px;
		background: var(--tile);
		overflow: hidden;
	}
	.cover img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
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

	@media (max-width: 700px) {
		.bar {
			left: 0;
			bottom: var(--mobile-nav-height);
		}
		.mini-viz {
			display: none;
		}
	}
</style>
