<script lang="ts">
	import CoverTile from '$lib/components/CoverTile.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function hrefFor(kind: string, id: number): string {
		const segment = kind === 'book' ? 'books' : kind === 'album' ? 'albums' : 'podcasts';
		return `/library/${segment}/${id}`;
	}
</script>

<PageTitle title="Favoriten" />

<div class="content" style="--a: var(--music)">
	<h1>Favoriten</h1>

	{#if data.items.length > 0}
		<h2>Titel &amp; Alben</h2>
		<div class="grid-6">
			{#each data.items as item (item.id)}
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
				<a href={hrefFor(item.kind, item.id)}>
					<CoverTile
						kind={item.kind}
						coverUrl={item.coverPath ? `/items/${item.id}/cover` : null}
						title={item.title}
						subtitle={item.artist ?? item.author ?? ''}
					/>
				</a>
			{/each}
		</div>
	{/if}

	{#if data.tracks.length > 0}
		<h2>Songs</h2>
		<div class="table" role="table" aria-label="Songs">
			{#each data.tracks as row (row.track.id)}
				<ListRow href={hrefFor(row.item.kind, row.item.id)}>
					<span class="title">{row.track.title ?? row.item.title}</span>
					<span class="cell">{row.item.title}</span>
				</ListRow>
			{/each}
		</div>
	{/if}

	{#if data.items.length === 0 && data.tracks.length === 0}
		<p class="empty">Noch keine Favoriten.</p>
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
	h2 {
		font: 600 13px var(--font-sans);
		color: var(--dim);
		margin: 20px 0 10px;
	}
	.grid-6 {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 22px 16px;
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
