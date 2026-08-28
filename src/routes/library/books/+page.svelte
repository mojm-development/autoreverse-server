<script lang="ts">
	import { resolve } from '$app/paths';
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import ListRow from '$lib/components/ListRow.svelte';
	import SortToggle from '$lib/components/SortToggle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function sortHref(target: string): string {
		const params = new SvelteURLSearchParams();
		if (data.q) params.set('q', data.q);
		params.set('sort', target);
		return `${resolve('/library/books')}?${params}`;
	}

	type Status = 'listening' | 'unstarted' | 'finished';
	function statusOf(bookId: number): Status {
		const p = data.progress[bookId];
		if (!p) return 'unstarted';
		if (p.finished) return 'finished';
		return 'listening';
	}

	let filter = $state<'all' | Status>('all');
	const filtered = $derived(
		filter === 'all' ? data.books : data.books.filter((b) => statusOf(b.id) === filter)
	);

	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}
	function percentOf(bookId: number): number {
		const p = data.progress[bookId];
		const total = data.durations[bookId] ?? 0;
		if (!p || total === 0) return 0;
		return Math.round((p.position / total) * 100);
	}
</script>

<div class="content" style="--a: var(--book)">
	<header>
		<h1>
			{data.series ? `Serie ${data.series}` : 'Bibliothek'}
			<span class="count mono">{data.total}</span>
		</h1>
	</header>

	<div class="pills">
		<button class="pill" class:active={filter === 'all'} onclick={() => (filter = 'all')}
			>Alle</button
		>
		<button
			class="pill"
			class:active={filter === 'listening'}
			onclick={() => (filter = 'listening')}>Wird gehört</button
		>
		<button
			class="pill"
			class:active={filter === 'unstarted'}
			onclick={() => (filter = 'unstarted')}>Nicht begonnen</button
		>
		<button class="pill" class:active={filter === 'finished'} onclick={() => (filter = 'finished')}
			>Beendet</button
		>
	</div>

	<div class="toolbar">
		<form method="GET" class="search-form">
			<input type="hidden" name="sort" value={data.sort} />
			<input type="search" name="q" value={data.q} placeholder="Bibliothek durchsuchen" />
		</form>
		{#if data.sortable}
			<span class="sort-label mono">sortiert:</span>
			<SortToggle
				current={data.sort}
				options={data.sortOptions.map((o) => ({ ...o, href: sortHref(o.key) }))}
			/>
		{/if}
	</div>

	<div class="table" role="table" aria-label="Hörbücher">
		<div class="table-head" role="row">
			<span></span><span>Titel</span><span>Autor</span><span>Sprecher</span><span>Fortschritt</span
			><span>Länge</span>
		</div>
		{#each filtered as book (book.id)}
			<ListRow href="/library/books/{book.id}">
				<span
					class="cover-thumb"
					style={book.coverPath ? `background-image: url(/items/${book.id}/cover)` : ''}
				></span>
				<span class="title">{book.title}</span>
				<span class="cell">{book.author ?? '—'}</span>
				<span class="cell">{book.narrator ?? '—'}</span>
				<span class="cell mono">{percentOf(book.id)} %</span>
				<span class="cell mono">{formatDuration(data.durations[book.id] ?? 0)}</span>
			</ListRow>
		{/each}
	</div>
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	header {
		display: flex;
		align-items: baseline;
		gap: 10px;
		margin-bottom: 14px;
	}
	.count {
		color: var(--faint);
		font-size: 13px;
	}
	.pills {
		display: flex;
		gap: 6px;
		margin-bottom: 16px;
	}
	.toolbar {
		display: flex;
		align-items: center;
		gap: 16px;
		margin-bottom: 20px;
	}
	.search-form input {
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
		font: 400 12.5px var(--font-sans);
	}
	.sort-label {
		color: var(--faint);
		font-size: 11px;
		margin-left: auto;
	}
	.table {
		display: flex;
		flex-direction: column;
	}
	.table-head {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 32px;
		padding: 0 12px;
		color: var(--faint);
		font: 600 10px var(--font-sans);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		border-bottom: 1px solid var(--line);
	}
	.table-head span:first-child {
		width: 36px;
	}
	.cover-thumb {
		width: 36px;
		height: 36px;
		flex: none;
		border-radius: var(--radius-sm);
		background-color: var(--tile);
		background-size: cover;
		background-position: center;
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
</style>
