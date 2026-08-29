<script lang="ts">
	import { getContext } from 'svelte';
	import { resolve } from '$app/paths';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import PodcastSearch from '$lib/components/PodcastSearch.svelte';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import EpisodeCard from '$lib/components/EpisodeCard.svelte';
	import { invalidateAll } from '$app/navigation';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import { relativeDay } from '$lib/dates';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let downloading = $state<number | null>(null);

	// An episode rarely carries its own artwork; fall back to the show's, the way the rail
	// already does — a background image leaves an empty tile instead of a broken icon.
	const showsWithCover = $derived(
		new Set(data.podcasts.filter((p) => p.cover_path).map((p) => p.id))
	);
	function coverStyle(episode: { id: number; cover_path: string | null; podcast_id: number }) {
		if (episode.cover_path) return `background-image: url("/items/${episode.id}/cover")`;
		if (showsWithCover.has(episode.podcast_id))
			return `background-image: url("/items/${episode.podcast_id}/cover")`;
		return '';
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

<PageTitle title={data.showNew ? 'Neue Folgen' : 'Podcasts'} />

<div class="layout" style="--a: var(--podcast)">
	<PodcastRail podcasts={data.podcasts} />
	<div class="content">
		{#if data.showNew}
			<header class="head">
				<div>
					<h1>Neue Folgen</h1>
					<p class="sub">
						{data.episodes.length === 0
							? 'Nichts Ungehörtes'
							: `${data.episodes.length} ${data.episodes.length === 1 ? 'Folge' : 'Folgen'} aus ${new Set(data.episodes.map((e) => e.podcast_id)).size} ${new Set(data.episodes.map((e) => e.podcast_id)).size === 1 ? 'Abo' : 'Abos'}`}
					</p>
				</div>
				<a class="pill" href={resolve('/library/podcasts')}>Alle Abos</a>
			</header>
			{#if data.episodes.length === 0}
				<div class="blank">
					<span class="blank-icon"><Icon name="podcast" /></span>
					<p class="blank-title">Alles gehört</p>
					<p class="blank-text">
						Neue Folgen deiner Abos erscheinen hier, sobald der Feed sie ausliefert.
					</p>
				</div>
			{:else}
				<ul class="episodes">
					{#each data.episodes as episode (episode.id)}
						<EpisodeCard
							title={episode.title}
							date={relativeDay(episode.published_at)}
							duration={episode.duration}
							downloaded={episode.downloaded}
							position={episode.position}
							coverStyle={coverStyle(episode)}
							show={{
								title: episode.podcast_title,
								href: resolve('/library/podcasts/[id]', { id: String(episode.podcast_id) })
							}}
							downloading={downloading === episode.id}
							onPlay={() => player.play(episode.id)}
							onDownload={() => download(episode.id)}
						/>
					{/each}
				</ul>
			{/if}
		{:else if data.podcasts.length > 0}
			<p class="empty">Wähle einen Podcast aus der Liste.</p>
		{:else}
			<p class="empty">Noch keine Podcast-Abos.</p>
		{/if}
		{#if data.user?.isAdmin}
			<section class="add-feed">
				<h2>Podcast abonnieren</h2>
				<p class="add-hint">Suche nach einem Namen oder füge die Adresse eines Feeds ein.</p>
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
	.content {
		flex: 1;
		min-width: 0;
		padding: 24px 32px 32px;
		max-width: 980px;
	}
	.head {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 16px;
		margin-bottom: 18px;
	}
	.head h1 {
		font: 600 22px var(--font-sans);
		margin: 0;
	}
	.sub {
		margin: 3px 0 0;
		color: var(--faint);
		font-size: 12px;
	}

	/* One card per episode: the title gets room to breathe, everything else is a
	   quiet second line, and playing is one obvious button. */
	.episodes {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.blank {
		border: 1px dashed var(--line);
		border-radius: var(--radius-lg);
		padding: 32px 24px;
		text-align: center;
	}
	.blank-icon {
		display: inline-grid;
		place-items: center;
		width: 44px;
		height: 44px;
		border-radius: 50%;
		font-size: 22px;
		color: var(--a);
		background: color-mix(in oklab, var(--a) 14%, transparent);
	}
	.blank-title {
		margin: 12px 0 4px;
		font: 600 14px var(--font-sans);
	}
	.blank-text {
		margin: 0;
		color: var(--faint);
		font-size: 12px;
	}
	.empty {
		color: var(--faint);
	}

	.add-feed {
		margin-top: 28px;
		padding: 18px;
		border-radius: var(--radius-lg);
		border: 1px solid var(--line);
		background: var(--panel);
	}
	.add-feed h2 {
		font: 600 14px var(--font-sans);
		margin: 0;
	}
	.add-hint {
		margin: 4px 0 12px;
		color: var(--faint);
		font-size: 12px;
	}

	@media (max-width: 700px) {
		.content {
			padding: 18px 16px 24px;
		}
	}
</style>
