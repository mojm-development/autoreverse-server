<script lang="ts">
	import { onMount } from 'svelte';
	import { browser } from '$app/environment';
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';

	type Result = {
		name: string;
		author: string | null;
		feed_url: string;
		artwork_url: string | null;
		episode_count: number | null;
	};

	let { onSubscribed, initialQuery = '' }: { onSubscribed: () => void; initialQuery?: string } =
		$props();

	let query = $state('');
	let results = $state<Result[]>([]);
	let loading = $state(false);
	let subscribing = $state<string | null>(null);
	let subscribed = $state<Record<string, number>>({});
	let error = $state('');
	let searched = $state(false);

	function coverStyle(url: string | null): string {
		if (!url) return '';
		const safe = url.replaceAll('\\', '%5C').replaceAll('"', '%22');
		return `background-image: url("${safe}")`;
	}

	onMount(() => {
		if (!initialQuery.trim()) return;
		query = initialQuery;
		void search();
	});

	function syncUrl() {
		if (!browser) return;
		const url = new URL(page.url);
		if (query.trim()) url.searchParams.set('q', query);
		else url.searchParams.delete('q');
		replaceState(url, page.state);
	}

	async function search() {
		if (!query.trim()) {
			results = [];
			searched = false;
			syncUrl();
			return;
		}
		syncUrl();
		loading = true;
		error = '';
		try {
			const res = await fetch(`/podcasts/search?q=${encodeURIComponent(query)}`);
			const body = await res.json();
			if (!res.ok) {
				error = body.detail ?? 'Suche fehlgeschlagen.';
				results = [];
			} else {
				results = body.results;
			}
		} catch {
			error = 'Suche fehlgeschlagen.';
			results = [];
		} finally {
			loading = false;
			searched = true;
		}
	}

	async function subscribe(feedUrl: string) {
		subscribing = feedUrl;
		try {
			const res = await fetch('/podcasts', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ feed_url: feedUrl })
			});
			const body = await res.json().catch(() => ({}));
			if (!res.ok) {
				error = body.detail ?? 'Abonnieren fehlgeschlagen.';
				return;
			}
			subscribed = { ...subscribed, [feedUrl]: body.id };
			onSubscribed();
		} finally {
			subscribing = null;
		}
	}
</script>

<div class="podcast-search">
	<form
		class="search-form"
		onsubmit={(e) => {
			e.preventDefault();
			search();
		}}
	>
		<input bind:value={query} placeholder="Podcast suchen" />
		<button type="submit" disabled={loading}>{loading ? '…' : 'Suchen'}</button>
	</form>

	{#if error}
		<p class="error">{error}</p>
	{/if}

	{#if results.length > 0}
		<ul class="results">
			{#each results as r (r.feed_url)}
				<li>
					<a
						class="open"
						href="/library/podcasts/preview?feed={encodeURIComponent(
							r.feed_url
						)}&q={encodeURIComponent(query)}"
					>
						<span class="cover" style={coverStyle(r.artwork_url)}></span>
						<span class="meta">
							<span class="name">{r.name}</span>
							{#if r.author}<span class="author">{r.author}</span>{/if}
							{#if r.episode_count !== null}
								<span class="episodes mono">{r.episode_count} Folgen</span>
							{/if}
						</span>
					</a>
					{#if subscribed[r.feed_url]}
						<a class="secondary small subscribed" href="/library/podcasts/{subscribed[r.feed_url]}">
							Abonniert ✓
						</a>
					{:else}
						<button
							type="button"
							class="secondary small"
							disabled={subscribing === r.feed_url}
							onclick={() => subscribe(r.feed_url)}
						>
							{subscribing === r.feed_url ? '…' : 'Abonnieren'}
						</button>
					{/if}
				</li>
			{/each}
		</ul>
	{:else if searched && !loading && !error}
		<p class="empty">Keine Treffer.</p>
	{/if}
</div>

<style>
	.podcast-search {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.search-form {
		display: flex;
		gap: 8px;
		max-width: 460px;
		font-size: 14px;
	}
	.search-form input {
		flex: 1;
		min-width: 0;
	}
	.error {
		color: var(--music);
		font-size: 11.5px;
	}
	.empty {
		color: var(--faint);
		font-size: 11.5px;
	}
	.results {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
		gap: 10px;
	}
	.results li {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 10px;
		border-radius: var(--radius-md, 8px);
		border: 1px solid transparent;
	}
	.results li:hover {
		background: var(--panel);
		border-color: var(--line);
	}
	.cover {
		width: 104px;
		height: 104px;
		flex: none;
		border-radius: 10px;
		background: var(--tile) center/cover;
	}
	.open {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
		gap: 14px;
		color: inherit;
	}
	.meta {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.name {
		font: 500 17px/1.35 var(--font-sans);
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		overflow: hidden;
	}
	.author {
		font-size: 14px;
		color: var(--dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.episodes {
		font-size: 12.5px;
		color: var(--faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.secondary.small {
		flex: none;
		height: 36px;
		padding: 0 18px;
		font-size: 14px;
	}
	.subscribed {
		display: inline-flex;
		align-items: center;
		white-space: nowrap;
	}
</style>
