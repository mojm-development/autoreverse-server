<script lang="ts">
	import { enhance } from '$app/forms';
	import { getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let feedUrl = $state('');

	type Filter = 'all' | 'unheard' | 'started' | 'downloaded';
	let filter = $state<Filter>('all');
	function statusOf(episodeId: number): 'unheard' | 'started' | 'finished' {
		const p = data.progress[episodeId];
		if (!p) return 'unheard';
		return p.finished ? 'finished' : 'started';
	}
	const filtered = $derived(
		data.episodes.filter((e) => {
			if (filter === 'all') return true;
			if (filter === 'downloaded') return e.id in data.durations;
			if (filter === 'unheard') return statusOf(e.id) === 'unheard';
			if (filter === 'started') return statusOf(e.id) === 'started';
			return true;
		})
	);
	const unheardCount = $derived(data.episodes.filter((e) => statusOf(e.id) === 'unheard').length);
	const downloadedCount = $derived(data.episodes.filter((e) => e.id in data.durations).length);

	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}
	function relativeTime(iso: Date | string | null): string {
		if (!iso) return 'nie';
		const days = Math.floor((Date.now() - new Date(iso).getTime()) / 86_400_000);
		if (days <= 0) return 'heute';
		if (days === 1) return 'gestern';
		return `vor ${days} Tagen`;
	}

	async function subscribe() {
		await fetch('/podcasts', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ feed_url: feedUrl })
		});
		feedUrl = '';
		location.reload();
	}
	async function refresh() {
		await fetch(`/podcasts/${data.podcast.id}/refresh`, { method: 'POST' });
		location.reload();
	}
	async function unsubscribe() {
		await fetch(`/podcasts/${data.podcast.id}`, { method: 'DELETE' });
		location.href = '/library/podcasts';
	}
	function playLatest() {
		if (data.episodes.length > 0) void player.play(data.episodes[0].id);
	}
</script>

<div class="layout" style="--a: var(--podcast)">
	<PodcastRail podcasts={data.podcasts} activeId={data.podcast.id}>
		{#if data.user?.isAdmin}
			<form
				class="add-feed"
				onsubmit={(e) => {
					e.preventDefault();
					subscribe();
				}}
			>
				<input type="url" bind:value={feedUrl} placeholder="Feed-URL hinzufügen" required />
				<button type="submit">Abonnieren</button>
			</form>
		{/if}
	</PodcastRail>

	<div class="content">
		<div class="hero">
			<div
				class="cover-lg"
				style={data.podcast.coverPath
					? `background-image: url(/items/${data.podcast.id}/cover)`
					: ''}
			></div>
			<div class="meta">
				<span class="eyebrow">Podcast</span>
				<h1>{data.podcast.title}</h1>
				<p class="subline">
					{data.episodes.length} Folgen · zuletzt geprüft {relativeTime(data.podcast.lastChecked)}
				</p>
				<div class="actions">
					<button class="primary" onclick={playLatest}
						><Icon name="play" /> Neueste abspielen</button
					>
					{#if data.user?.isAdmin}
						<button class="outline" onclick={refresh}><Icon name="download" /> Aktualisieren</button
						>
						<button class="outline" onclick={unsubscribe}>Abo kündigen</button>
					{/if}
				</div>
				{#if data.user?.isAdmin}
					<form class="keep" method="POST" action="?/keep" use:enhance>
						<label for="keep">Folgen vorhalten</label>
						<input
							id="keep"
							name="keep"
							type="number"
							min="0"
							max={data.keepMax}
							placeholder="Standard ({data.keepDefault})"
							value={data.podcast.keepEpisodes ?? ''}
						/>
						<button type="submit" class="secondary">Übernehmen</button>
						<span class="keep-hint">leer = Standard ({data.keepDefault})</span>
					</form>
				{/if}
			</div>
		</div>

		<div class="pills">
			<button class="pill" class:active={filter === 'all'} onclick={() => (filter = 'all')}
				>Alle</button
			>
			<button class="pill" class:active={filter === 'unheard'} onclick={() => (filter = 'unheard')}
				>Ungehört · {unheardCount}</button
			>
			<button class="pill" class:active={filter === 'started'} onclick={() => (filter = 'started')}
				>Angefangen</button
			>
			<button
				class="pill"
				class:active={filter === 'downloaded'}
				onclick={() => (filter = 'downloaded')}>Geladen · {downloadedCount}</button
			>
		</div>

		<div class="table" role="table" aria-label="Folgen">
			{#each filtered as episode (episode.id)}
				{@const downloaded = episode.id in data.durations}
				<ListRow label="{episode.title} abspielen" onclick={() => player.play(episode.id)}>
					<Icon name={statusOf(episode.id) === 'finished' ? 'heart-filled' : 'play'} />
					<span class="title">{episode.title}</span>
					<span class="cell mono"
						>{episode.publishedAt
							? new Date(episode.publishedAt).toLocaleDateString('de-DE')
							: ''}</span
					>
					<span class="cell mono">{formatDuration(data.durations[episode.id] ?? 0)}</span>
					<span class="cell status">{downloaded ? 'geladen' : 'streamen'}</span>
				</ListRow>
			{/each}
		</div>
	</div>
</div>

<style>
	.layout {
		display: flex;
		min-height: 100%;
	}
	.add-feed {
		margin-top: 14px;
		padding: 10px;
		border: 1px dashed var(--line-strong);
		border-radius: 8px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.add-feed input {
		height: 30px;
		padding: 0 8px;
		border-radius: 6px;
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
		font-size: 12px;
	}
	.add-feed button {
		height: 28px;
		border-radius: 6px;
		border: none;
		background: var(--a);
		color: var(--bg);
		font: 500 12px var(--font-sans);
	}
	.content {
		flex: 1;
		padding: 24px 32px;
	}
	.hero {
		display: flex;
		gap: 20px;
		margin-bottom: 24px;
	}
	.cover-lg {
		width: 104px;
		height: 104px;
		flex: none;
		border-radius: var(--radius-lg);
		background: var(--tile);
		background-size: cover;
		background-position: center;
	}
	.meta {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 5px;
	}
	h1 {
		font: 600 22px/1.2 var(--font-sans);
		margin: 0;
	}
	.subline {
		color: var(--dim);
		font-size: 12.5px;
		margin: 0;
	}
	.keep {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 12px;
		flex-wrap: wrap;
	}
	.keep label {
		font-size: 12px;
		color: var(--dim);
	}
	.keep input {
		width: 130px;
	}
	.keep-hint {
		font-size: 11px;
		color: var(--faint);
	}
	.actions {
		display: flex;
		gap: 10px;
		margin-top: 10px;
	}
	.primary,
	.outline {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		height: 32px;
		padding: 0 14px;
		border-radius: var(--radius-pill);
		font: 500 12.5px var(--font-sans);
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
	.pills {
		display: flex;
		gap: 6px;
		margin-bottom: 16px;
	}
	.table {
		display: flex;
		flex-direction: column;
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
	.status {
		color: var(--faint);
	}
</style>
