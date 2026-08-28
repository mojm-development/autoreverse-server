<script lang="ts">
	import { resolve } from '$app/paths';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import type { PageData, ActionData } from './$types';
	let { data, form }: { data: PageData; form: ActionData } = $props();

	const backHref = $derived(
		data.query
			? `${resolve('/library/podcasts')}?q=${encodeURIComponent(data.query)}`
			: resolve('/library/podcasts')
	);

	function coverStyle(url: string | null): string {
		if (!url) return '';
		const safe = url.replaceAll('\\', '%5C').replaceAll('"', '%22');
		return `background-image: url("${safe}")`;
	}

	function formatDate(iso: Date | string | null): string {
		if (!iso) return '—';
		return new Date(iso).toLocaleDateString('de-DE', {
			day: '2-digit',
			month: '2-digit',
			year: 'numeric'
		});
	}
	function formatDuration(seconds: number | null): string {
		if (!seconds) return '';
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}
</script>

<div class="layout" style="--a: var(--podcast)">
	<PodcastRail podcasts={data.podcasts} />

	<div class="content">
		<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
		<a class="back" href={backHref}>← Zurück zur Suche</a>

		{#if data.loadError}
			<p class="error">{data.loadError}</p>
			<p class="feed-url mono">{data.feedUrl}</p>
		{:else if data.feed}
			<div class="hero">
				<div class="cover-lg" style={coverStyle(data.feed.imageUrl)}></div>
				<div class="meta">
					<span class="eyebrow">Podcast</span>
					<h1>{data.feed.title}</h1>
					<p class="subline">{data.feed.episodeCount} Folgen im Feed</p>
					<div class="actions">
						{#if data.subscribedId !== null}
							<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
							<a class="primary" href="/library/podcasts/{data.subscribedId}">Bereits abonniert</a>
						{:else}
							<form method="POST">
								<button type="submit" class="primary">Abonnieren</button>
							</form>
						{/if}
					</div>
					{#if form?.error}<p class="error">{form.error}</p>{/if}
				</div>
			</div>

			{#if data.feed.description}
				<p class="description">{data.feed.description}</p>
			{/if}

			<h2>Folgen</h2>
			<div class="episodes">
				{#each data.feed.episodes as episode (episode.guid)}
					<div class="episode">
						<div class="episode-head">
							<span class="episode-title">{episode.title}</span>
							<span class="episode-date mono">{formatDate(episode.publishedAt)}</span>
							{#if episode.durationSeconds}
								<span class="episode-duration mono">{formatDuration(episode.durationSeconds)}</span>
							{/if}
						</div>
						{#if episode.description}
							<p class="episode-description">{episode.description}</p>
						{/if}
					</div>
				{/each}
			</div>
			{#if data.feed.episodeCount > data.feed.episodes.length}
				<p class="more">
					… und {data.feed.episodeCount - data.feed.episodes.length} weitere Folgen. Nach dem Abonnieren
					sind alle da.
				</p>
			{/if}
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
		padding: 24px 32px;
		max-width: 1100px;
	}
	.back {
		display: inline-block;
		margin-bottom: 18px;
		color: var(--dim);
		font-size: 13px;
	}
	.back:hover {
		color: var(--text);
	}
	.hero {
		display: flex;
		gap: 22px;
		margin-bottom: 22px;
	}
	.cover-lg {
		width: 168px;
		height: 168px;
		flex: none;
		border-radius: var(--radius-lg);
		background: var(--tile) center/cover;
	}
	.meta {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 6px;
		min-width: 0;
	}
	h1 {
		font: 600 28px/1.2 var(--font-sans);
		margin: 0;
	}
	.subline {
		color: var(--dim);
		font-size: 14px;
		margin: 0;
	}
	.actions {
		display: flex;
		gap: 10px;
		margin-top: 12px;
	}
	.primary {
		display: inline-flex;
		align-items: center;
		height: 36px;
		padding: 0 20px;
		border-radius: var(--radius-pill);
		background: var(--a);
		color: var(--bg);
		border: none;
		font: 500 14px var(--font-sans);
	}
	.description {
		color: var(--dim);
		font: 400 14px/1.6 var(--font-sans);
		margin: 0 0 26px;
		max-width: 78ch;
	}
	h2 {
		font: 600 16px var(--font-sans);
		margin: 0 0 12px;
	}
	.episodes {
		display: flex;
		flex-direction: column;
	}
	.episode {
		padding: 12px 0;
		border-top: 1px solid var(--line);
	}
	.episode-head {
		display: flex;
		align-items: baseline;
		gap: 12px;
	}
	.episode-title {
		flex: 1;
		min-width: 0;
		font: 500 14px/1.4 var(--font-sans);
	}
	.episode-date,
	.episode-duration {
		flex: none;
		color: var(--faint);
		font-size: 11.5px;
	}
	.episode-description {
		margin: 5px 0 0;
		color: var(--dim);
		font: 400 13px/1.55 var(--font-sans);
		max-width: 92ch;
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		overflow: hidden;
	}
	.more {
		margin-top: 16px;
		color: var(--faint);
		font-size: 12.5px;
	}
	.error {
		color: var(--music);
		font-size: 13px;
	}
	.feed-url {
		color: var(--faint);
		font-size: 12px;
		word-break: break-all;
	}
</style>
