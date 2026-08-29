<script lang="ts">
	import { getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let isFavorite = $derived(data.isFavorite);

	const totalDuration = $derived(data.tracks.reduce((sum, t) => sum + t.duration, 0));

	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}

	async function toggleFavorite() {
		const next = !isFavorite;
		isFavorite = next;
		await fetch(`/favorites/items/${data.album.id}`, { method: next ? 'POST' : 'DELETE' });
	}

	async function shufflePlay() {
		await player.play(data.album.id);
		const current = player.current;
		if (!current) return;
		const tracks = current.tracks;
		for (let i = tracks.length - 1; i > 0; i--) {
			const j = Math.floor(Math.random() * (i + 1));
			[tracks[i], tracks[j]] = [tracks[j], tracks[i]];
		}
		player.reloadCurrentTrack();
	}
</script>

<PageTitle title={data.album.title} />

<div class="content" style="--a: var(--music)">
	<div class="hero">
		<div class="cover">
			{#if data.album.coverPath}
				<img src="/items/{data.album.id}/cover" alt="" />
			{/if}
		</div>
		<div class="meta">
			<span class="eyebrow">Album{data.album.year ? ` · ${data.album.year}` : ''}</span>
			<h1>{data.album.title}</h1>
			<p class="subline">
				{data.album.artist ?? ''} · {data.tracks.length} Titel · {formatDuration(totalDuration)}
			</p>
			<div class="actions">
				<button class="primary" onclick={() => player.play(data.album.id)}>
					<Icon name="play" /> Abspielen
				</button>
				<button class="outline" onclick={shufflePlay}>
					<Icon name="shuffle" /> Zufall
				</button>
				<button
					class="icon-btn"
					aria-label={isFavorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten hinzufügen'}
					aria-pressed={isFavorite}
					onclick={toggleFavorite}
				>
					<Icon name={isFavorite ? 'heart-filled' : 'heart'} />
				</button>
				<button class="icon-btn" aria-label="Weitere Optionen" disabled>
					<Icon name="queue" />
				</button>
			</div>
		</div>
	</div>

	<div class="table" role="table" aria-label="Titel">
		<div class="table-head" role="row">
			<span></span><span>Titel</span><span>Interpret</span><span></span><span>Dauer</span>
		</div>
		{#each data.tracks as track, i (track.id)}
			{@const playing =
				player.current !== null &&
				player.current.itemId === data.album.id &&
				player.current.trackIndex === i}
			<ListRow
				ariaCurrent={playing}
				label="{track.title ?? `Titel ${i + 1}`} abspielen"
				onclick={() => player.playTrackAt(data.album.id, i)}
			>
				<span class="index mono">
					{#if playing}<Icon name="play" />{:else}{i + 1}{/if}
				</span>
				<span class="title">{track.title ?? `Titel ${i + 1}`}</span>
				<span class="cell">{data.album.artist ?? '—'}</span>
				<span class="cell fav above">
					<button
						class="icon-btn small"
						aria-label="Titel favorisieren"
						onclick={() => fetch(`/favorites/tracks/${track.id}`, { method: 'POST' })}
					>
						<Icon name="heart" />
					</button>
				</span>
				<span class="cell mono">{formatDuration(track.duration)}</span>
			</ListRow>
		{/each}
	</div>
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	.hero {
		display: flex;
		gap: 24px;
		margin-bottom: 32px;
	}
	.cover {
		width: 186px;
		height: 186px;
		flex: none;
		border-radius: var(--radius-lg);
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
		overflow: hidden;
	}
	.cover img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.meta {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 6px;
	}
	h1 {
		font: 600 26px/1.2 var(--font-sans);
		margin: 0;
	}
	.subline {
		color: var(--dim);
		font-size: 13px;
		margin: 0;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-top: 14px;
	}
	.primary,
	.outline {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		height: 36px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		font: 500 13px var(--font-sans);
		border: none;
	}
	.primary {
		background: var(--a);
		color: var(--bg);
	}
	.outline {
		background: transparent;
		border: 1px solid var(--line-strong);
		color: var(--text);
	}
	.icon-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 36px;
		height: 36px;
		border-radius: 50%;
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
	}
	.icon-btn[aria-pressed='true'] {
		color: var(--a);
		border-color: var(--a);
	}
	.icon-btn.small {
		width: 26px;
		height: 26px;
		border: none;
	}
	.table {
		display: flex;
		flex-direction: column;
	}
	.table-head {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 32px;
		padding: 0 12px;
		color: var(--faint);
		font: 600 10px var(--font-sans);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		border-bottom: 1px solid var(--line);
	}
	.table-head span:first-child {
		width: 24px;
	}
	.index {
		width: 24px;
		flex: none;
		color: var(--faint);
		display: flex;
		align-items: center;
	}
	.title {
		flex: 2;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cell {
		flex: 1;
		color: var(--dim);
		font-size: 12px;
	}
	.fav {
		flex: none;
		width: 26px;
	}
	:global([role='row'][aria-current='true']) {
		background: color-mix(in oklab, var(--a) 12%, transparent);
	}
	:global([role='row'][aria-current='true']) .index {
		color: var(--a);
	}
</style>
