<script lang="ts">
	import { enhance } from '$app/forms';
	import { getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import EpisodeCard from '$lib/components/EpisodeCard.svelte';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import { relativeDay } from '$lib/dates';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let feedUrl = $state('');
	let confirmingUnsubscribe = $state(false);
	let refreshing = $state(false);

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
			return statusOf(e.id) === filter;
		})
	);
	const unheardCount = $derived(data.episodes.filter((e) => statusOf(e.id) === 'unheard').length);
	const startedCount = $derived(data.episodes.filter((e) => statusOf(e.id) === 'started').length);
	const downloadedCount = $derived(data.episodes.filter((e) => e.id in data.durations).length);
	const FILTERS: { key: Filter; label: string; count: () => number }[] = [
		{ key: 'all', label: 'Alle', count: () => data.episodes.length },
		{ key: 'unheard', label: 'Ungehört', count: () => unheardCount },
		{ key: 'started', label: 'Angefangen', count: () => startedCount },
		{ key: 'downloaded', label: 'Geladen', count: () => downloadedCount }
	];

	// Feed order is newest first, so the oldest episode is number one.
	const numbers = $derived(new Map(data.episodes.map((e, i) => [e.id, data.episodes.length - i])));

	/** Where to pick up: the newest episode already started, else the newest unheard one. */
	const continueEpisode = $derived(
		data.episodes.find((e) => statusOf(e.id) === 'started') ??
			data.episodes.find((e) => statusOf(e.id) === 'unheard') ??
			null
	);

	function relativeTime(iso: Date | string | null): string {
		if (!iso) return 'nie';
		const label = relativeDay(typeof iso === 'string' ? iso : iso.toISOString());
		return label ? label.toLowerCase() : 'nie';
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
		refreshing = true;
		try {
			await fetch(`/podcasts/${data.podcast.id}/refresh`, { method: 'POST' });
			location.reload();
		} finally {
			refreshing = false;
		}
	}
	async function unsubscribe() {
		await fetch(`/podcasts/${data.podcast.id}`, { method: 'DELETE' });
		location.href = '/library/podcasts';
	}
	function playLatest() {
		const episode = continueEpisode ?? data.episodes[0];
		if (episode) void player.play(episode.id);
	}
</script>

<PageTitle title={data.podcast.title} />

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
		<header class="hero">
			<div
				class="cover-lg"
				style={data.podcast.coverPath
					? `background-image: url(/items/${data.podcast.id}/cover)`
					: ''}
			></div>
			<div class="hero-meta">
				<span class="eyebrow">Podcast</span>
				<h1>{data.podcast.title}</h1>
				<p class="subline">
					{data.episodes.length}
					{data.episodes.length === 1 ? 'Folge' : 'Folgen'}
					{#if unheardCount > 0}<span class="dot">·</span> {unheardCount} ungehört{/if}
					<span class="dot">·</span> zuletzt geprüft {relativeTime(data.podcast.lastChecked)}
				</p>
				<div class="actions">
					<button class="primary" onclick={playLatest} disabled={data.episodes.length === 0}>
						<Icon name="play-filled" />
						{continueEpisode && statusOf(continueEpisode.id) === 'started'
							? 'Fortsetzen'
							: 'Neueste abspielen'}
					</button>
					{#if data.user?.isAdmin}
						<button class="outline" onclick={refresh} disabled={refreshing}>
							<Icon name="download" />
							{refreshing ? 'Wird geprüft …' : 'Aktualisieren'}
						</button>
						{#if confirmingUnsubscribe}
							<button class="danger" onclick={unsubscribe}>Wirklich kündigen</button>
							<button class="outline" onclick={() => (confirmingUnsubscribe = false)}
								>Abbrechen</button
							>
						{:else}
							<button class="outline" onclick={() => (confirmingUnsubscribe = true)}>
								Abo kündigen
							</button>
						{/if}
					{/if}
				</div>
			</div>
		</header>

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
				<span class="keep-hint">
					Ältere Folgen werden gelöscht, sobald mehr geladen sind. Leer = Standard ({data.keepDefault}).
				</span>
			</form>
		{/if}

		<div class="pills" role="group" aria-label="Folgen filtern">
			{#each FILTERS as option (option.key)}
				<button
					class="pill"
					class:active={filter === option.key}
					aria-pressed={filter === option.key}
					onclick={() => (filter = option.key)}
				>
					{option.label}
					<span class="pill-count mono">{option.count()}</span>
				</button>
			{/each}
		</div>

		{#if filtered.length === 0}
			<p class="empty">
				{data.episodes.length === 0
					? 'Dieser Feed hat noch keine Folgen geliefert.'
					: 'Keine Folge passt zu diesem Filter.'}
			</p>
		{:else}
			<ul class="episodes">
				{#each filtered as episode (episode.id)}
					{@const progress = data.progress[episode.id]}
					<EpisodeCard
						title={episode.title}
						index={numbers.get(episode.id)}
						date={relativeDay(
							episode.publishedAt ? new Date(episode.publishedAt).toISOString() : null
						)}
						duration={data.durations[episode.id] ?? progress?.duration ?? 0}
						downloaded={episode.id in data.durations}
						position={progress?.position ?? 0}
						finished={progress?.finished ?? false}
						onPlay={() => player.play(episode.id)}
					/>
				{/each}
			</ul>
		{/if}
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
		min-width: 0;
		padding: 24px 32px 32px;
		max-width: 980px;
	}
	.hero {
		display: flex;
		gap: 20px;
		align-items: flex-end;
		padding-bottom: 20px;
		margin-bottom: 18px;
		border-bottom: 1px solid var(--line);
	}
	.cover-lg {
		width: 136px;
		height: 136px;
		flex: none;
		border-radius: var(--radius-lg);
		background: var(--tile) center/cover;
		box-shadow: 0 12px 30px -18px rgb(0 0 0 / 0.7);
	}
	.hero-meta {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 5px;
		min-width: 0;
	}
	h1 {
		font: 600 26px/1.15 var(--font-sans);
		margin: 0;
	}
	.subline {
		color: var(--dim);
		font-size: 12.5px;
		margin: 0;
	}
	.dot {
		opacity: 0.5;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin-top: 12px;
	}
	.primary,
	.outline,
	.danger {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		height: 34px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		font: 500 12.5px var(--font-sans);
		border: none;
	}
	.primary {
		background: var(--a);
		color: var(--bg);
		font-size: 15px;
	}
	.primary:hover:not(:disabled) {
		filter: brightness(1.08);
	}
	.primary:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	.outline {
		background: transparent;
		border: 1px solid var(--line-strong);
		color: var(--text);
	}
	.outline:hover:not(:disabled) {
		background: var(--panel);
	}
	/* Cancelling a subscription deletes downloaded episodes — it asks first. */
	.danger {
		background: var(--danger);
		color: #fff;
	}

	.keep {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
		margin-bottom: 18px;
		padding: 12px 14px;
		border-radius: var(--radius-lg);
		border: 1px solid var(--line);
		background: var(--panel);
	}
	.keep label {
		font-size: 12px;
		color: var(--dim);
	}
	.keep input {
		width: 130px;
	}
	.keep-hint {
		flex: 1;
		min-width: 220px;
		font-size: 11px;
		color: var(--faint);
	}

	.pills {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin-bottom: 14px;
	}
	.pill-count {
		margin-left: 6px;
		font-size: 10.5px;
		color: var(--faint);
	}
	.pill.active .pill-count {
		color: inherit;
		opacity: 0.75;
	}
	.episodes {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.empty {
		color: var(--faint);
	}

	@media (max-width: 700px) {
		.content {
			padding: 18px 16px 24px;
		}
		.hero {
			gap: 14px;
		}
		.cover-lg {
			width: 92px;
			height: 92px;
		}
		h1 {
			font-size: 20px;
		}
	}
</style>
