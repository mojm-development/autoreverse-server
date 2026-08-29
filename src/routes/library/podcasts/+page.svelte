<script lang="ts">
	import { getContext } from 'svelte';
	import { resolve } from '$app/paths';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import PodcastSearch from '$lib/components/PodcastSearch.svelte';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import Icon from '$lib/components/Icon.svelte';
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

	function formatDuration(seconds: number): string {
		if (!seconds) return '';
		const hours = Math.floor(seconds / 3600);
		const minutes = Math.round((seconds % 3600) / 60);
		return hours >= 1 ? `${hours} Std ${minutes} min` : `${minutes} min`;
	}
	/** What is left of an episode someone already started. */
	function remaining(duration: number, position: number): string {
		const left = Math.max(0, duration - position);
		return `noch ${formatDuration(left) || '<1 min'}`;
	}
	function progressOf(episode: { duration: number; position: number }): number {
		if (!episode.duration || !episode.position) return 0;
		return Math.min(100, (episode.position / episode.duration) * 100);
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
						{@const started = episode.position > 0}
						<li class="episode">
							<a
								class="cover"
								href={resolve('/library/podcasts/[id]', { id: String(episode.podcast_id) })}
								aria-label={episode.podcast_title}
								style={coverStyle(episode)}
							></a>
							<div class="body">
								<h2 class="title">{episode.title}</h2>
								<p class="meta">
									<a
										class="show"
										href={resolve('/library/podcasts/[id]', { id: String(episode.podcast_id) })}
										>{episode.podcast_title}</a
									>
									<span class="dot">·</span>
									<span class="mono">{relativeDay(episode.published_at)}</span>
									{#if episode.duration}
										<span class="dot">·</span>
										<span class="mono">{formatDuration(episode.duration)}</span>
									{/if}
									{#if episode.downloaded}
										<span class="badge-soft">geladen</span>
									{/if}
								</p>
								{#if started}
									<p class="progress-line">
										<span class="track"
											><span class="fill" style="width: {progressOf(episode)}%"></span></span
										>
										<span class="left mono">{remaining(episode.duration, episode.position)}</span>
									</p>
								{/if}
							</div>
							<div class="actions">
								{#if !episode.downloaded}
									<button
										class="ghost-btn"
										aria-label="Folge herunterladen"
										title="Folge herunterladen"
										disabled={downloading === episode.id}
										onclick={() => download(episode.id)}
									>
										<Icon name="download" />
									</button>
								{/if}
								<button
									class="play"
									aria-label="{episode.title} {started ? 'fortsetzen' : 'abspielen'}"
									title={started ? 'Fortsetzen' : 'Abspielen'}
									onclick={() => player.play(episode.id)}
								>
									<Icon name="play-filled" />
								</button>
							</div>
						</li>
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
	.episode {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 12px 14px;
		border-radius: var(--radius-md, 12px);
		border: 1px solid var(--line);
		background: var(--panel);
	}
	.episode:hover {
		border-color: color-mix(in oklab, var(--a) 40%, var(--line));
		background: var(--panel-hi, var(--panel));
	}
	.cover {
		width: 56px;
		height: 56px;
		flex: none;
		border-radius: 8px;
		background: var(--tile) center/cover;
	}
	.body {
		flex: 1;
		min-width: 0;
	}
	.title {
		margin: 0;
		font: 600 14px/1.35 var(--font-sans);
		/* Two lines, then ellipsis: podcast titles are long and cutting at one loses the point. */
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.meta {
		margin: 4px 0 0;
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 6px;
		font-size: 11.5px;
		color: var(--faint);
	}
	.show {
		color: var(--dim);
		max-width: 220px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.show:hover {
		color: var(--a);
	}
	.dot {
		opacity: 0.5;
	}
	.badge-soft {
		padding: 1px 7px;
		border-radius: var(--radius-pill, 999px);
		background: color-mix(in oklab, var(--a) 16%, transparent);
		color: color-mix(in oklab, var(--a) 70%, var(--text));
		font-size: 10.5px;
	}
	.progress-line {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 7px 0 0;
	}
	.track {
		flex: 1;
		max-width: 260px;
		height: 3px;
		border-radius: 2px;
		background: var(--line);
		overflow: hidden;
	}
	.fill {
		display: block;
		height: 100%;
		background: var(--a);
	}
	.left {
		font-size: 10.5px;
		color: var(--faint);
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: none;
	}
	.ghost-btn {
		width: 34px;
		height: 34px;
		padding: 0;
		display: grid;
		place-items: center;
		border-radius: 50%;
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
		font-size: 16px;
	}
	.ghost-btn:hover:not(:disabled) {
		color: var(--text);
		border-color: var(--line-strong);
	}
	.ghost-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.play {
		width: 40px;
		height: 40px;
		padding: 0;
		border: none;
		border-radius: 50%;
		background: var(--a);
		color: var(--bg);
		font-size: 20px;
		display: grid;
		place-items: center;
	}
	.play:hover {
		filter: brightness(1.08);
	}

	.blank {
		border: 1px dashed var(--line);
		border-radius: var(--radius-md, 12px);
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
		border-radius: var(--radius-md, 12px);
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
		.cover {
			width: 46px;
			height: 46px;
		}
	}
</style>
