<script lang="ts">
	import { getContext } from 'svelte';
	import { resolve } from '$app/paths';
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import CoverTile from '$lib/components/CoverTile.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import SortToggle from '$lib/components/SortToggle.svelte';
	import ViewToggle from '$lib/components/ViewToggle.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import SelectionBar from '$lib/components/SelectionBar.svelte';
	import BulkEditor from '$lib/components/BulkEditor.svelte';
	import { invalidateAll } from '$app/navigation';
	import { humanDuration } from '$lib/dates';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);

	let selected = $state<number[]>([]);
	let bulkOpen = $state(false);
	let lastPicked = $state<number | null>(null);

	function pick(id: number, event: MouseEvent) {
		const index = filtered.findIndex((book) => book.id === id);
		if (event.shiftKey && lastPicked !== null) {
			const from = filtered.findIndex((book) => book.id === lastPicked);
			if (from >= 0 && index >= 0) {
				const [start, end] = from < index ? [from, index] : [index, from];
				selected = [...new Set([...selected, ...filtered.slice(start, end + 1).map((b) => b.id)])];
				lastPicked = id;
				return;
			}
		}
		selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
		lastPicked = id;
	}
	function clearSelection() {
		selected = [];
		lastPicked = null;
	}

	function href(params: Record<string, string>): string {
		const search = new SvelteURLSearchParams();
		if (data.q) search.set('q', data.q);
		if (data.series) search.set('series', data.series);
		// The author scopes the series: two writers can use the same series name.
		if (data.seriesAuthor) search.set('author', data.seriesAuthor);
		if (data.sort !== 'series') search.set('sort', data.sort);
		if (data.view !== 'grid') search.set('view', data.view);
		for (const [key, value] of Object.entries(params)) {
			if (value) search.set(key, value);
			else search.delete(key);
		}
		const query = search.toString();
		return query ? `${resolve('/library/books')}?${query}` : resolve('/library/books');
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
	function countOf(status: Status): number {
		return data.books.filter((b) => statusOf(b.id) === status).length;
	}
	const FILTERS = $derived([
		{ key: 'all' as const, label: 'Alle', count: data.books.length },
		{ key: 'listening' as const, label: 'Wird gehört', count: countOf('listening') },
		{ key: 'unstarted' as const, label: 'Nicht begonnen', count: countOf('unstarted') },
		{ key: 'finished' as const, label: 'Beendet', count: countOf('finished') }
	]);

	function percentOf(bookId: number): number {
		const p = data.progress[bookId];
		const total = data.durations[bookId] ?? 0;
		if (!p || total === 0) return 0;
		if (p.finished) return 100;
		return Math.round((p.position / total) * 100);
	}
	/** What a listener wants to know mid-book: not the percentage, the time left. */
	function remainingOf(bookId: number): string {
		const p = data.progress[bookId];
		const total = data.durations[bookId] ?? 0;
		if (!p || p.finished || total === 0) return '';
		return `noch ${humanDuration(Math.max(0, total - p.position))}`;
	}
	function bandOf(book: { series: string | null; seriesIndex: number | null }): string {
		if (!book.series) return '';
		return book.seriesIndex ? `${book.series} · Band ${book.seriesIndex}` : book.series;
	}
</script>

<PageTitle title={data.series ?? 'Bibliothek'} />

<div class="content" style="--a: var(--book)">
	<header>
		<h1>
			{data.series ? data.series : 'Bibliothek'}
			<span class="count mono">{data.total}</span>
		</h1>
		{#if data.series && data.seriesAuthor}
			<span class="head-author">{data.seriesAuthor}</span>
		{/if}
	</header>

	<div class="toolbar">
		<form method="GET" class="search-form">
			<input type="hidden" name="sort" value={data.sort} />
			<input type="hidden" name="view" value={data.view} />
			{#if data.series}<input type="hidden" name="series" value={data.series} />{/if}
			{#if data.seriesAuthor}<input type="hidden" name="author" value={data.seriesAuthor} />{/if}
			<span class="search-field">
				<Icon name="search" />
				<input type="search" name="q" value={data.q} placeholder="Bibliothek durchsuchen" />
			</span>
		</form>
		{#if data.sortable}
			<SortToggle
				current={data.sort}
				options={data.sortOptions.map((o) => ({ ...o, href: href({ sort: o.key }) }))}
			/>
		{/if}
		<div class="spacer"></div>
		<ViewToggle
			view={data.view}
			gridHref={href({ view: 'grid' })}
			listHref={href({ view: 'list' })}
		/>
	</div>

	<div class="pills" role="group" aria-label="Nach Fortschritt filtern">
		{#each FILTERS as option (option.key)}
			<button
				class="pill"
				class:active={filter === option.key}
				aria-pressed={filter === option.key}
				onclick={() => (filter = option.key)}
			>
				{option.label}
				<span class="pill-count mono">{option.count}</span>
			</button>
		{/each}
	</div>

	{#if data.q || data.series}
		<div class="chips">
			{#if data.q}
				<span class="chip">
					Suche: {data.q}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
					<a href={href({ q: '' })} aria-label="Suche aufheben">×</a>
				</span>
			{/if}
			{#if data.series}
				<span class="chip">
					Serie: {data.series}{#if data.seriesAuthor}
						· {data.seriesAuthor}{/if}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
					<a href={href({ series: '', author: '' })} aria-label="Serienfilter aufheben">×</a>
				</span>
			{/if}
		</div>
	{/if}

	{#if filtered.length === 0}
		<p class="empty">
			{data.books.length === 0
				? data.q
					? `Nichts gefunden für „${data.q}“.`
					: 'Noch keine Hörbücher in der Bibliothek.'
				: 'Kein Hörbuch in diesem Filter.'}
		</p>
	{:else if data.view === 'grid'}
		<div class="grid-6">
			{#each filtered as book (book.id)}
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
				<a href="/library/books/{book.id}" class="tile-link">
					<CoverTile
						kind="book"
						coverUrl={book.coverPath ? `/items/${book.id}/cover` : null}
						title={book.title}
						subtitle={book.author ?? ''}
						progress={percentOf(book.id)}
						selected={selected.includes(book.id)}
						selectLabel="{book.title} auswählen"
						onSelect={data.user?.isAdmin ? (event) => pick(book.id, event) : undefined}
						playLabel="{book.title} {statusOf(book.id) === 'listening'
							? 'fortsetzen'
							: 'abspielen'}"
						onPlay={() => player.play(book.id)}
					/>
				</a>
			{/each}
		</div>
	{:else}
		<div class="table" role="table" aria-label="Hörbücher">
			<div class="table-head" role="row">
				<span></span><span>Titel</span><span>Autor</span><span>Fortschritt</span><span>Länge</span>
			</div>
			{#each filtered as book (book.id)}
				<ListRow href="/library/books/{book.id}">
					<span
						class="cover-thumb"
						style={book.coverPath ? `background-image: url(/items/${book.id}/cover)` : ''}
					></span>
					<span class="title-cell">
						<span class="title">{book.title}</span>
						{#if bandOf(book)}<span class="band">{bandOf(book)}</span>{/if}
					</span>
					<span class="cell">{book.author ?? ''}</span>
					<span class="cell progress-cell">
						{#if statusOf(book.id) === 'finished'}
							<span class="done">beendet</span>
						{:else if percentOf(book.id) > 0}
							<span class="track"
								><span class="fill" style="width: {percentOf(book.id)}%"></span></span
							>
							<span class="left mono">{remainingOf(book.id)}</span>
						{/if}
					</span>
					<span class="cell mono right">{humanDuration(data.durations[book.id] ?? 0) || '—'}</span>
				</ListRow>
			{/each}
		</div>
	{/if}
</div>

{#if data.user?.isAdmin}
	<SelectionBar count={selected.length} onClear={clearSelection} onEdit={() => (bulkOpen = true)} />
{/if}

{#if bulkOpen}
	<BulkEditor
		kind="book"
		ids={selected}
		count={selected.length}
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
	.head-author {
		color: var(--dim);
		font-size: 12.5px;
	}
	.toolbar {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
		margin-bottom: 12px;
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
		width: 200px;
		border: none;
		background: transparent;
		color: var(--text);
		font: 400 12.5px var(--font-sans);
		outline: none;
	}
	.search-field input::-webkit-search-cancel-button {
		display: none;
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
	.table-head span:first-child {
		width: 36px;
	}
	.table-head span:nth-child(2) {
		flex: 2;
	}
	.table-head span:nth-child(3),
	.table-head span:nth-child(4) {
		flex: 1;
	}
	.table-head span:nth-child(5) {
		flex: 1;
		text-align: right;
	}
	.cover-thumb {
		width: 36px;
		height: 36px;
		flex: none;
		border-radius: var(--radius-sm);
		background: var(--tile) center/cover;
	}
	.title-cell {
		flex: 2;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 1px;
	}
	.title {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	/* Books are sorted by series by default — say which volume this is. */
	.band {
		font-size: 10.5px;
		color: var(--faint);
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
	.progress-cell {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.track {
		flex: 1;
		max-width: 90px;
		height: 3px;
		border-radius: 2px;
		background: var(--line);
		overflow: hidden;
	}
	.fill {
		display: block;
		height: 100%;
		background: var(--a);
	}
	.left {
		font-size: 10.5px;
		color: var(--faint);
	}
	.done {
		font-size: 11px;
		color: var(--faint);
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
