<script lang="ts">
	import { resolve } from '$app/paths';
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import CoverTile from '$lib/components/CoverTile.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import ViewToggle from '$lib/components/ViewToggle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function viewHref(target: 'grid' | 'list'): string {
		const params = new SvelteURLSearchParams();
		if (data.q) params.set('q', data.q);
		if (data.sort !== 'title') params.set('sort', data.sort);
		params.set('view', target);
		return `${resolve('/library/albums')}?${params}`;
	}

	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}
</script>

<div class="content" style="--a: var(--music)">
	<header>
		<h1>
			{data.artist ? `Alben von ${data.artist}` : 'Alben'}
			<span class="count mono">{data.total}</span>
		</h1>
	</header>

	<div class="pills">
		<span class="pill active">Alle</span>
		<!-- /library/favorites doesn't exist as a route yet (lands in Task 42); resolve() needs a
			statically-known route id, same rationale as the src/lib/components exemption in eslint.config.js. -->
		<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
		<a class="pill" href="/library/favorites">Favoriten</a>
		<a class="pill" href={resolve('/library/albums?sort=added')}>Zuletzt dazu</a>
		<!-- /library/artists doesn't exist as a route yet (lands in Task 42); same rationale as above. -->
		<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
		<a class="pill" href="/library/artists">Interpreten</a>
	</div>

	<div class="toolbar">
		<form method="GET" class="search-form">
			<input type="hidden" name="view" value={data.view} />
			<input type="hidden" name="sort" value={data.sort} />
			<input type="search" name="q" value={data.q} placeholder="Alben durchsuchen" />
		</form>
		<span class="sort-label mono">sortiert: {data.sortLabel}</span>
		<ViewToggle view={data.view} gridHref={viewHref('grid')} listHref={viewHref('list')} />
	</div>

	{#if data.view === 'grid'}
		<div class="grid-6">
			{#each data.albums as album (album.id)}
				<!-- /library/albums/[id] doesn't exist as a route yet (lands in a later task);
					resolve() needs a statically-known route id, same rationale as the
					src/lib/components exemption in eslint.config.js. -->
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
				<a href="/library/albums/{album.id}" class="tile-link">
					<CoverTile
						kind="album"
						coverUrl={album.coverPath ? `/items/${album.id}/cover` : null}
						title={album.title}
						subtitle={album.artist ?? ''}
					/>
				</a>
			{/each}
		</div>
	{:else}
		<div class="table" role="table" aria-label="Alben">
			<div class="table-head" role="row">
				<span></span><span></span><span>Titel</span><span>Interpret</span><span>Jahr</span><span
					>Genre</span
				><span>Dauer</span>
			</div>
			{#each data.albums as album, i (album.id)}
				<ListRow href="/library/albums/{album.id}">
					<span class="mono index">{i + 1}</span>
					<span
						class="cover-thumb"
						style={album.coverPath ? `background-image: url(/items/${album.id}/cover)` : ''}
					></span>
					<span class="title">{album.title}</span>
					<span class="cell">{album.artist ?? '—'}</span>
					<span class="cell">{album.year ?? '—'}</span>
					<span class="cell">—</span>
					<span class="mono cell">{formatDuration(data.durations[album.id] ?? 0)}</span>
				</ListRow>
			{/each}
		</div>
	{/if}
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
	.grid-6 {
		display: grid;
		grid-template-columns: repeat(6, 1fr);
		gap: 16px;
	}
	.tile-link {
		color: inherit;
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
	.table-head span:nth-child(1) {
		width: 24px;
	}
	.table-head span:nth-child(2) {
		width: 36px;
	}
	.index {
		width: 24px;
		flex: none;
		color: var(--faint);
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
