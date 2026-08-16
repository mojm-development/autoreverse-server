<script lang="ts">
	import { resolve } from '$app/paths';
	let query = $state('');
	let results = $state<Array<{ name: string; author: string; feed_url: string }>>([]);

	async function search() {
		if (!query.trim()) {
			results = [];
			return;
		}
		const res = await fetch(`/podcasts/search?q=${encodeURIComponent(query)}`);
		if (res.ok) results = (await res.json()).results;
	}
	async function subscribe(feedUrl: string) {
		await fetch('/podcasts', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ feed_url: feedUrl })
		});
		results = [];
		query = '';
	}
</script>

<h1>Podcast-Abos</h1>
<form
	onsubmit={(e) => {
		e.preventDefault();
		search();
	}}
	class="search-form"
>
	<input bind:value={query} placeholder="Podcast-Verzeichnis durchsuchen" />
	<button type="submit">Suchen</button>
</form>

{#each results as r (r.feed_url)}
	<div class="result">
		<span class="title">{r.name}</span>
		<span class="cell">{r.author}</span>
		<button onclick={() => subscribe(r.feed_url)}>Abonnieren</button>
	</div>
{/each}

<p class="hint">
	Aktuelle Abos und deren Verwaltung (aktualisieren/kündigen) unter
	<a href={resolve('/library/podcasts')}>Podcasts</a>.
</p>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
	}
	.search-form {
		display: flex;
		gap: 8px;
		margin-bottom: 14px;
	}
	.search-form input {
		flex: 1;
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
	}
	.result {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px 4px;
		border-bottom: 1px solid var(--line);
	}
	.title {
		flex: 1;
		font-size: 13px;
	}
	.cell {
		color: var(--dim);
		font-size: 12px;
	}
	.hint {
		color: var(--faint);
		font-size: 12px;
		margin-top: 16px;
	}
</style>
