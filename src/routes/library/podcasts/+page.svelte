<script lang="ts">
	import { getContext } from 'svelte';
	import { resolve } from '$app/paths';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import PodcastSearch from '$lib/components/PodcastSearch.svelte';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import { invalidateAll } from '$app/navigation';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let downloading = $state<number | null>(null);

	function formatDate(value: string | null): string {
		return value ? new Date(value).toLocaleDateString('de-DE') : '';
	}
	function formatDuration(seconds: number): string {
		if (!seconds) return '';
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}
	async function download(episodeId: number) {
		downloading = episodeId;
		try {
			await fetch(`/episodes/${episodeId}/download`, { method: 'POST' });
			await invalidateAll();
		} finally {
			downloading = null;
		}
	}
</script>

<div class="layout" style="--a: var(--podcast)">
	<PodcastRail podcasts={data.podcasts} />
	<div class="content">
		{#if data.showNew}
			<header class="head">
				<h1>Neue Folgen <span class="count mono">{data.episodes.length}</span></h1>
				<a class="pill" href={resolve('/library/podcasts')}>Alle Abos</a>
			</header>
			{#if data.episodes.length === 0}
				<p class="empty">Keine ungehörten Folgen seit deinen Abos.</p>
			{:else}
				<div class="table" role="table" aria-label="Neue Folgen">
					{#each data.episodes as episode (episode.id)}
						<ListRow label="{episode.title} abspielen" onclick={() => player.play(episode.id)}>
							<Icon name="play" />
							<span class="title">{episode.title}</span>
							<span class="cell show">{episode.podcast_title}</span>
							<span class="cell mono">{formatDate(episode.published_at)}</span>
							<span class="cell mono">{formatDuration(episode.duration)}</span>
							<span class="cell state above">
								{#if episode.downloaded}
									geladen
								{:else}
									<button
										class="icon-btn small"
										aria-label="Folge herunterladen"
										disabled={downloading === episode.id}
										onclick={() => download(episode.id)}
									>
										<Icon name="download" />
									</button>
								{/if}
							</span>
						</ListRow>
					{/each}
				</div>
			{/if}
		{:else if data.podcasts.length > 0}
			<p class="empty">Wähle einen Podcast aus der Liste.</p>
		{:else}
			<p class="empty">Noch keine Podcast-Abos.</p>
		{/if}
		{#if data.user?.isAdmin}
			<section class="add-feed">
				<h2>Podcast abonnieren</h2>
				<PodcastSearch initialQuery={data.query} onSubscribed={() => invalidateAll()} />
			</section>
		{/if}
	</div>
</div>

<style>
	.layout {
		display: flex;
		min-height: 100%;
	}
	.add-feed {
		margin-top: 28px;
		padding-top: 20px;
		border-top: 1px solid var(--line);
		max-width: 1100px;
	}
	.add-feed h2 {
		font: 600 15px var(--font-sans);
		margin-bottom: 12px;
	}
	.content {
		flex: 1;
		min-width: 0;
		padding: 24px 32px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		margin-bottom: 16px;
	}
	.head h1 {
		font: 600 20px var(--font-sans);
		margin: 0;
	}
	.count {
		color: var(--faint);
		font-size: 13px;
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
		min-width: 0;
	}
	.show {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.state {
		flex: none;
		width: 70px;
		text-align: right;
	}
	.empty {
		color: var(--faint);
	}
</style>
