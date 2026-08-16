<script lang="ts">
	import Icon from '$lib/components/Icon.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function accent(kind: string): string {
		if (kind === 'book') return 'book';
		if (kind === 'album') return 'music';
		return 'podcast'; // podcast, episode
	}

	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}

	function status(itemId: number, fallbackDuration: number | null): string {
		const p = data.progress[itemId];
		if (!p) return fallbackDuration ? formatDuration(fallbackDuration) : '';
		if (p.finished) return 'beendet';
		const pct = p.duration > 0 ? Math.round((p.position / p.duration) * 100) : 0;
		return `${pct} % gehört`;
	}

	function subtitle(row: {
		kind: string;
		author: string | null;
		artist: string | null;
		series: string | null;
	}): string {
		if (row.kind === 'book') return [row.author, row.series].filter(Boolean).join(' · ');
		return row.artist ?? '';
	}

	const totalHits = $derived(
		data.books.length + data.albums.length + data.podcasts.length + data.tracks.length
	);

	function clearSearch(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			window.location.assign('/library/search');
		}
	}
</script>

<div class="content" style="--a: var(--music)">
	<form method="GET" class="search-bar">
		<Icon name="search" />
		<input
			type="search"
			name="q"
			value={data.q}
			placeholder="Suchen…"
			onkeydown={clearSearch}
			autofocus
		/>
		<span class="hint mono">esc</span>
	</form>

	{#if data.q}
		<p class="meta mono">{totalHits} Treffer in {data.total} Items · {data.elapsedMs} ms</p>

		<div class="pills">
			<span class="pill active">Alle</span>
			<span class="pill">Hörbücher · {data.books.length}</span>
			<span class="pill">Alben · {data.albums.length}</span>
			<span class="pill">Titel · {data.tracks.length}</span>
			<span class="pill">Folgen · {data.podcasts.length}</span>
		</div>

		{#if data.books.length > 0}
			<section>
				<div class="section-head">
					<span class="eyebrow">Hörbücher</span><span class="count mono">{data.books.length}</span>
				</div>
				{#each data.books as row (row.id)}
					<ListRow href="/library/books/{row.id}">
						<span class="dot" style="background: var(--{accent(row.kind)})"></span>
						<span class="title">{row.title}</span>
						<span class="row-meta">{subtitle(row)}</span>
						<span class="status mono">{status(row.id, null)}</span>
					</ListRow>
				{/each}
			</section>
		{/if}

		{#if data.albums.length > 0 || data.tracks.length > 0}
			<section>
				<div class="section-head">
					<span class="eyebrow">Alben &amp; Titel</span><span class="count mono"
						>{data.albums.length + data.tracks.length}</span
					>
				</div>
				{#each data.albums as row (row.id)}
					<ListRow href="/library/albums/{row.id}">
						<span class="dot" style="background: var(--{accent(row.kind)})"></span>
						<span class="title">{row.title}</span>
						<span class="row-meta">{subtitle(row)}</span>
						<span class="status mono">{status(row.id, null)}</span>
					</ListRow>
				{/each}
				{#each data.tracks as track (track.id)}
					<ListRow>
						<span class="dot" style="background: var(--music)"></span>
						<span class="title">{track.title}</span>
						<span class="row-meta">{track.item_title}</span>
						<span class="status mono">{formatDuration(track.duration)}</span>
					</ListRow>
				{/each}
			</section>
		{/if}

		{#if data.podcasts.length > 0}
			<section>
				<div class="section-head">
					<span class="eyebrow">Podcast-Folgen</span><span class="count mono"
						>{data.podcasts.length}</span
					>
				</div>
				{#each data.podcasts as row (row.id)}
					<ListRow href="/library/podcasts/{row.id}">
						<span class="dot" style="background: var(--podcast)"></span>
						<span class="title">{row.title}</span>
						<span class="row-meta">{subtitle(row)}</span>
						<span class="status mono">{status(row.id, null)}</span>
					</ListRow>
				{/each}
			</section>
		{/if}

		{#if data.artists.length > 0}
			<section>
				<div class="section-head">
					<span class="eyebrow">Interpreten &amp; Autoren</span><span class="count mono"
						>{data.artists.length}</span
					>
				</div>
				{#each data.artists as row (row.name)}
					<ListRow href="/library/albums?artist={encodeURIComponent(row.name)}">
						<span class="dot" style="background: var(--{row.role === 'author' ? 'book' : 'music'})"
						></span>
						<span class="title">{row.name}</span>
						<span class="row-meta">{row.role === 'author' ? 'Autor:in' : 'Interpret:in'}</span>
						<span class="status mono">{row.work_count}</span>
					</ListRow>
				{/each}
			</section>
		{/if}
	{/if}
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	.search-bar {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 40px;
		padding: 0 12px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		max-width: 480px;
	}
	.search-bar input {
		flex: 1;
		border: none;
		background: transparent;
		color: var(--text);
		font: 400 13px var(--font-sans);
		outline: none;
	}
	.hint {
		color: var(--faint);
		font-size: 10px;
		border: 1px solid var(--line);
		border-radius: 4px;
		padding: 1px 5px;
	}
	.meta {
		color: var(--dim);
		font-size: 11.5px;
		margin: 14px 0;
	}
	.pills {
		display: flex;
		gap: 6px;
		margin-bottom: 20px;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		height: 26px;
		padding: 0 11px;
		border-radius: var(--radius-pill);
		font: 500 11.5px/1 var(--font-sans);
		border: 1px solid var(--line-strong);
		color: var(--dim);
	}
	.pill.active {
		background: var(--a);
		color: var(--bg);
		border-color: transparent;
	}
	section {
		margin-bottom: 26px;
	}
	.section-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		padding-bottom: 6px;
		border-bottom: 1px solid var(--line);
		margin-bottom: 4px;
	}
	.count {
		color: var(--faint);
		font-size: 11px;
	}
	.dot {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		flex: none;
	}
	.title {
		font: 500 13px var(--font-sans);
		flex: 1;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-meta {
		color: var(--dim);
		font-size: 11.5px;
		flex: 1;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.status {
		color: var(--faint);
		font-size: 11px;
		flex: none;
	}
</style>
