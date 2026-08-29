<script lang="ts">
	import { getContext } from 'svelte';
	import { resolve } from '$app/paths';
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import CoverTile from '$lib/components/CoverTile.svelte';
	import SortToggle from '$lib/components/SortToggle.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import ViewToggle from '$lib/components/ViewToggle.svelte';
	import InfiniteScroll from '$lib/components/InfiniteScroll.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import SelectionBar from '$lib/components/SelectionBar.svelte';
	import BulkEditor from '$lib/components/BulkEditor.svelte';
	import { invalidateAll } from '$app/navigation';
	import { humanDuration } from '$lib/dates';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);

	// Bulk selection. `allMatching` means "everything this filter finds", which is sent
	// to the API as the filter itself rather than as 1599 ids.
	let selected = $state<number[]>([]);
	let allMatching = $state(false);
	let bulkOpen = $state(false);
	let lastPicked = $state<number | null>(null);

	function pick(id: number, event: MouseEvent) {
		const index = albums.findIndex((album) => album.id === id);
		if (event.shiftKey && lastPicked !== null) {
			const from = albums.findIndex((album) => album.id === lastPicked);
			if (from >= 0 && index >= 0) {
				const [start, end] = from < index ? [from, index] : [index, from];
				const range = albums.slice(start, end + 1).map((album) => album.id);
				selected = [...new Set([...selected, ...range])];
				lastPicked = id;
				return;
			}
		}
		selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
		allMatching = false;
		lastPicked = id;
	}
	function clearSelection() {
		selected = [];
		allMatching = false;
		lastPicked = null;
	}
	const bulkFilter = $derived({
		kind: 'album',
		q: data.q || undefined,
		artist: data.artist || undefined,
		missing: data.missing || undefined
	});

	type Album = PageData['albums'][number];
	interface Loaded {
		key: string;
		items: Album[];
		durations: Record<number, number>;
		exhausted: boolean;
	}

	const EMPTY: Loaded = { key: '', items: [], durations: {}, exhausted: false };

	const filterKey = $derived([data.sort, data.q, data.artist, data.missing].join('\u0000'));

	let stored = $state<Loaded>(EMPTY);
	let loading = $state(false);

	const loaded = $derived(stored.key === filterKey ? stored : EMPTY);
	const albums = $derived([...data.albums, ...loaded.items]);
	const durations = $derived({ ...data.durations, ...loaded.durations });
	const done = $derived(!data.hasMore || loaded.exhausted);

	async function loadMore() {
		if (loading || done) return;
		const key = filterKey;
		const before = loaded;
		loading = true;
		const params = new SvelteURLSearchParams({
			kind: 'album',
			sort: data.sort,
			offset: String(data.albums.length + before.items.length),
			limit: String(data.pageSize)
		});
		if (data.q) params.set('q', data.q);
		if (data.missing) params.set('missing', 'true');
		try {
			const response = await fetch(`/library/more?${params}`);
			if (key !== filterKey) return;
			if (!response.ok) {
				stored = { ...before, key, exhausted: true };
				return;
			}
			const body = await response.json();
			if (key !== filterKey) return;
			const known = new Set([...data.albums, ...before.items].map((a) => a.id));
			stored = {
				key,
				items: [...before.items, ...body.items.filter((item: Album) => !known.has(item.id))],
				durations: { ...before.durations, ...body.durations },
				exhausted: !body.hasMore
			};
		} catch {
			stored = { ...before, key, exhausted: true };
		} finally {
			loading = false;
		}
	}

	function viewHref(target: 'grid' | 'list'): string {
		const params = new SvelteURLSearchParams();
		if (data.q) params.set('q', data.q);
		if (data.artist) params.set('artist', data.artist);
		if (data.missing) params.set('missing', 'true');
		if (data.sort !== 'title') params.set('sort', data.sort);
		params.set('view', target);
		return `${resolve('/library/albums')}?${params}`;
	}

	function listHref(params: Record<string, string>): string {
		const search = new SvelteURLSearchParams();
		if (data.q) search.set('q', data.q);
		if (data.artist) search.set('artist', data.artist);
		if (data.view !== 'grid') search.set('view', data.view);
		for (const [key, value] of Object.entries(params)) {
			if (value) search.set(key, value);
			else search.delete(key);
		}
		const query = search.toString();
		return query ? `${resolve('/library/albums')}?${query}` : resolve('/library/albums');
	}

	const SORTS = [
		{ key: 'title', label: 'Titel A–Z' },
		{ key: 'added', label: 'Zuletzt dazu' }
	];
</script>

<PageTitle
	title={data.artist ? `Alben von ${data.artist}` : data.missing ? 'Fehlende Alben' : 'Alben'}
/>

<div class="content" style="--a: var(--music)">
	<header>
		<h1>
			{data.artist ? `Alben von ${data.artist}` : data.missing ? 'Fehlende Alben' : 'Alben'}
			<span class="count mono">{data.total}</span>
		</h1>
	</header>

	<div class="toolbar">
		<form method="GET" class="search-form">
			<input type="hidden" name="view" value={data.view} />
			<input type="hidden" name="sort" value={data.sort} />
			{#if data.artist}<input type="hidden" name="artist" value={data.artist} />{/if}
			<span class="search-field">
				<Icon name="search" />
				<input type="search" name="q" value={data.q} placeholder="Alben durchsuchen" />
			</span>
		</form>
		<SortToggle
			current={data.sort}
			options={SORTS.map((o) => ({ ...o, href: listHref({ sort: o.key }) }))}
		/>
		<div class="spacer"></div>
		<ViewToggle view={data.view} gridHref={viewHref('grid')} listHref={viewHref('list')} />
	</div>

	{#if data.q || data.artist || data.missing}
		<div class="chips">
			{#if data.q}
				<span class="chip"
					>Suche: {data.q}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
					<a href={listHref({ q: '' })} aria-label="Suche aufheben">×</a></span
				>
			{/if}
			{#if data.artist}
				<span class="chip"
					>Interpret: {data.artist}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
					<a href={listHref({ artist: '' })} aria-label="Interpretenfilter aufheben">×</a></span
				>
			{/if}
			{#if data.missing}
				<span class="chip"
					>Fehlend <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
					<a href={listHref({ missing: '' })} aria-label="Filter aufheben">×</a></span
				>
			{/if}
		</div>
	{/if}

	{#if albums.length === 0}
		<p class="empty">
			{data.q ? `Nichts gefunden für „${data.q}“.` : 'Noch keine Alben in der Bibliothek.'}
		</p>
	{:else if data.view === 'grid'}
		<div class="grid-6">
			{#each albums as album (album.id)}
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
				<a href="/library/albums/{album.id}" class="tile-link">
					<CoverTile
						kind="album"
						coverUrl={album.coverPath ? `/items/${album.id}/cover` : null}
						title={album.title}
						subtitle={album.artist ?? ''}
						playLabel="{album.title} abspielen"
						onPlay={() => player.play(album.id)}
						selected={allMatching || selected.includes(album.id)}
						selectLabel="{album.title} auswählen"
						onSelect={data.user?.isAdmin ? (event) => pick(album.id, event) : undefined}
					/>
				</a>
			{/each}
		</div>
	{:else}
		<div class="table" role="table" aria-label="Alben">
			<div class="table-head" role="row">
				<span></span><span></span><span>Titel</span><span>Interpret</span><span>Jahr</span><span
					>Dauer</span
				>
			</div>
			{#each albums as album, i (album.id)}
				<ListRow href="/library/albums/{album.id}">
					<span class="index mono">
						<span class="number">{i + 1}</span>
						<button
							class="row-play"
							aria-label="{album.title} abspielen"
							onclick={(event) => {
								event.preventDefault();
								player.play(album.id);
							}}
						>
							<Icon name="play-filled" />
						</button>
					</span>
					<span
						class="cover-thumb"
						style={album.coverPath ? `background-image: url(/items/${album.id}/cover)` : ''}
					></span>
					<span class="title">{album.title}</span>
					<span class="cell">{album.artist ?? '—'}</span>
					<span class="cell mono">{album.year ?? '—'}</span>
					<span class="cell mono right">{humanDuration(durations[album.id] ?? 0) || '—'}</span>
				</ListRow>
			{/each}
		</div>
	{/if}

	<InfiniteScroll {done} {loading} onLoadMore={loadMore} label="Weitere Alben werden geladen" />
</div>

{#if data.user?.isAdmin}
	<SelectionBar
		count={selected.length}
		total={data.total}
		{allMatching}
		onSelectAll={() => (allMatching = true)}
		onClear={clearSelection}
		onEdit={() => (bulkOpen = true)}
	/>
{/if}

{#if bulkOpen}
	<BulkEditor
		kind="album"
		ids={allMatching ? undefined : selected}
		filter={allMatching ? bulkFilter : undefined}
		count={allMatching ? data.total : selected.length}
		onClose={() => (bulkOpen = false)}
		onApplied={async () => {
			clearSelection();
			await invalidateAll();
		}}
	/>
{/if}

<style>
	.content {
		padding: 24px 32px 32px;
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
	.toolbar {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
		margin-bottom: 14px;
	}
	.spacer {
		flex: 1;
	}
	.search-field {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--faint);
	}
	.search-field:focus-within {
		border-color: var(--line-strong);
		color: var(--dim);
	}
	.search-field input {
		width: 190px;
		border: none;
		background: transparent;
		color: var(--text);
		font: 400 12.5px var(--font-sans);
		outline: none;
	}
	.search-field input::-webkit-search-cancel-button {
		display: none;
	}

	/* What is filtering the list, and how to drop it — the old pill row mixed those
	   with links to other pages. */
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin-bottom: 14px;
	}
	.chip {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		height: 26px;
		padding: 0 6px 0 10px;
		border-radius: var(--radius-pill);
		border: 1px solid var(--line);
		background: var(--panel);
		font-size: 11.5px;
		color: var(--dim);
	}
	.chip a {
		display: grid;
		place-items: center;
		width: 16px;
		height: 16px;
		border-radius: 50%;
		color: var(--faint);
		font-size: 12px;
		line-height: 1;
	}
	.chip a:hover {
		background: var(--panel-hi);
		color: var(--text);
	}

	.grid-6 {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 22px 16px;
	}
	.tile-link {
		color: inherit;
		min-width: 0;
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
		width: 28px;
	}
	.table-head span:nth-child(2) {
		width: 36px;
	}
	.table-head span:nth-child(3) {
		flex: 2;
	}
	.table-head span:nth-child(4),
	.table-head span:nth-child(5) {
		flex: 1;
	}
	.table-head span:nth-child(6) {
		flex: 1;
		text-align: right;
	}
	/* The row number turns into a play button under the pointer, the way a track list does. */
	.index {
		position: relative;
		width: 28px;
		height: 28px;
		flex: none;
		display: grid;
		place-items: center;
		color: var(--faint);
	}
	.row-play {
		position: absolute;
		inset: 0;
		width: 28px;
		height: 28px;
		padding: 0;
		border: none;
		border-radius: 50%;
		background: var(--a);
		color: var(--bg);
		font-size: 14px;
		display: grid;
		place-items: center;
		opacity: 0;
	}
	:global(.row:hover) .row-play,
	.row-play:focus-visible {
		opacity: 1;
	}
	:global(.row:hover) .number {
		opacity: 0;
	}
	.cover-thumb {
		width: 36px;
		height: 36px;
		flex: none;
		border-radius: var(--radius-sm);
		background: var(--tile) center/cover;
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
		min-width: 0;
		color: var(--dim);
		font-size: 12px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.right {
		text-align: right;
	}
	.empty {
		color: var(--faint);
	}

	@media (max-width: 700px) {
		.content {
			padding: 18px 16px 24px;
		}
		.search-field input {
			width: 140px;
		}
	}
</style>
