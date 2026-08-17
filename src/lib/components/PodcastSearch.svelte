<script lang="ts">
	type Result = {
		name: string;
		author: string | null;
		feed_url: string;
		artwork_url: string | null;
		episode_count: number | null;
	};

	let { onSubscribed }: { onSubscribed: () => void } = $props();

	let query = $state('');
	let results = $state<Result[]>([]);
	let loading = $state(false);
	let subscribing = $state<string | null>(null);
	let error = $state('');
	let searched = $state(false);

	async function search() {
		if (!query.trim()) {
			results = [];
			searched = false;
			return;
		}
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
			if (!res.ok) {
				const body = await res.json().catch(() => ({}));
				error = body.detail ?? 'Abonnieren fehlgeschlagen.';
				return;
			}
			query = '';
			results = [];
			searched = false;
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
					<span class="cover" style={r.artwork_url ? `background-image:url(${r.artwork_url})` : ''}
					></span>
					<span class="meta">
						<span class="name">{r.name}</span>
						{#if r.author}<span class="author">{r.author}</span>{/if}
					</span>
					<button
						type="button"
						class="secondary small"
						disabled={subscribing === r.feed_url}
						onclick={() => subscribe(r.feed_url)}
					>
						{subscribing === r.feed_url ? '…' : 'Abonnieren'}
					</button>
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
		gap: 6px;
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
		display: flex;
		flex-direction: column;
		gap: 4px;
		max-height: 280px;
		overflow-y: auto;
	}
	.results li {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 4px;
		border-radius: var(--radius-sm);
	}
	.results li:hover {
		background: var(--panel);
	}
	.cover {
		width: 32px;
		height: 32px;
		flex: none;
		border-radius: 6px;
		background: var(--tile) center/cover;
	}
	.meta {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.name {
		font-size: 12px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.author {
		font-size: 10.5px;
		color: var(--faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.secondary.small {
		flex: none;
		height: 26px;
		padding: 0 10px;
		font-size: 11px;
	}
</style>
