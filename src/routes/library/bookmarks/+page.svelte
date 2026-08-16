<script lang="ts">
	import ListRow from '$lib/components/ListRow.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function formatHMS(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${h}:${pad(m)}:${pad(s)}`;
	}
	function hrefFor(kind: string, id: number): string {
		const segment = kind === 'book' ? 'books' : kind === 'album' ? 'albums' : 'podcasts';
		return `/library/${segment}/${id}`;
	}
</script>

<div class="content" style="--a: var(--book)">
	<h1>Lesezeichen <span class="count mono">{data.bookmarks.length}</span></h1>
	<div class="table" role="table" aria-label="Lesezeichen">
		{#each data.bookmarks as row (row.bookmark.id)}
			<ListRow href={hrefFor(row.item.kind, row.item.id)}>
				<span class="title">{row.item.title}</span>
				<span class="cell">{row.bookmark.title}</span>
				<span class="cell mono">{formatHMS(row.bookmark.position)}</span>
			</ListRow>
		{/each}
	</div>
	{#if data.bookmarks.length === 0}
		<p class="empty">Noch keine Lesezeichen.</p>
	{/if}
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
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
	}
	.cell {
		flex: 1;
		color: var(--dim);
		font-size: 12px;
	}
	.empty {
		color: var(--faint);
	}
</style>
