<script lang="ts">
	import Icon from '$lib/components/Icon.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	let query = $state('');
	const needle = $derived(query.trim().toLowerCase());
	const shown = $derived(
		needle
			? data.series.filter(
					(s) =>
						s.series.toLowerCase().includes(needle) ||
						(s.author ?? '').toLowerCase().includes(needle)
				)
			: data.series
	);

	/** One section per author: a series belongs to whoever wrote it. */
	const byAuthor = $derived(
		shown.reduce<{ author: string | null; entries: typeof shown }[]>((groups, entry) => {
			const last = groups[groups.length - 1];
			if (last && last.author === entry.author) last.entries.push(entry);
			else groups.push({ author: entry.author, entries: [entry] });
			return groups;
		}, [])
	);

	function seriesHref(entry: { series: string; author: string | null }): string {
		const series = `series=${encodeURIComponent(entry.series)}`;
		const author = entry.author ? `&author=${encodeURIComponent(entry.author)}` : '';
		return `/library/books?${series}${author}`;
	}
</script>

<PageTitle title="Serien" />

<div class="content" style="--a: var(--book)">
	<header>
		<h1>Serien <span class="count mono">{data.series.length}</span></h1>
		<span class="search-field">
			<Icon name="search" />
			<input
				type="search"
				placeholder="Serie oder Autor suchen"
				value={query}
				oninput={(e) => (query = e.currentTarget.value)}
			/>
		</span>
	</header>

	{#if shown.length === 0}
		<p class="empty">
			{data.series.length === 0
				? 'Keines deiner Hörbücher gehört zu einer Serie.'
				: `Keine Serie passt zu „${query}“.`}
		</p>
	{:else}
		{#each byAuthor as group (group.author ?? '\u0000')}
			<section class="author-group">
				<h2 class="author-name">
					{group.author ?? 'Ohne Autor'}
					<span class="count mono">{group.entries.length}</span>
				</h2>
				<ul class="grid">
					{#each group.entries as entry (entry.series)}
						<li>
							<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
							<a class="tile" href={seriesHref(entry)}>
								<span class="stack">
									{#each (entry.covers ?? []).slice(0, 3) as coverId, i (coverId)}
										<span
											class="spine"
											style="background-image: url(/items/{coverId}/cover); --i: {i}"
										></span>
									{/each}
									{#if (entry.covers ?? []).length === 0}
										<span class="spine empty-spine" style="--i: 0"></span>
									{/if}
								</span>
								<span class="body">
									<span class="name">{entry.series}</span>
									<span class="count-sub mono">
										{entry.count}
										{entry.count === 1 ? 'Band' : 'Bände'}
										{#if entry.finished_count > 0}· {entry.finished_count} beendet{/if}
									</span>
								</span>
							</a>
						</li>
					{/each}
				</ul>
			</section>
		{/each}
	{/if}
</div>

<style>
	.content {
		padding: 24px 32px 32px;
	}
	header {
		display: flex;
		align-items: center;
		gap: 16px;
		flex-wrap: wrap;
		margin-bottom: 20px;
	}
	h1 {
		font: 600 20px var(--font-sans);
		margin: 0;
	}
	.count {
		color: var(--faint);
		font-size: 13px;
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
		margin-left: auto;
	}
	.search-field:focus-within {
		border-color: var(--line-strong);
		color: var(--dim);
	}
	.search-field input {
		width: 220px;
		border: none;
		background: transparent;
		color: var(--text);
		font: 400 12.5px var(--font-sans);
		outline: none;
	}
	.search-field input::-webkit-search-cancel-button {
		display: none;
	}

	.author-group {
		margin-bottom: 26px;
	}
	.author-name {
		display: flex;
		align-items: baseline;
		gap: 8px;
		font: 600 13px var(--font-sans);
		color: var(--dim);
		margin: 0 0 10px;
		padding-bottom: 6px;
		border-bottom: 1px solid var(--line);
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: 10px;
	}
	.tile {
		height: 100%;
		box-sizing: border-box;
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 12px 14px;
		border-radius: var(--radius-lg);
		border: 1px solid var(--line);
		background: var(--panel);
		color: inherit;
	}
	.tile:hover {
		border-color: color-mix(in oklab, var(--a) 40%, var(--line));
		background: var(--panel-hi);
	}
	/* Three covers fanned out: a series is a shelf, not a word. */
	.stack {
		position: relative;
		/* Three 52px covers, each offset by 14px. */
		width: 80px;
		height: 52px;
		flex: none;
	}
	.spine {
		position: absolute;
		top: 0;
		left: calc(var(--i) * 14px);
		width: 52px;
		height: 52px;
		border-radius: var(--radius-sm);
		background: var(--tile) center/cover;
		box-shadow: 0 0 0 1px var(--line);
	}
	.empty-spine {
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
	}
	.body {
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.name {
		font: 600 13px var(--font-sans);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.count-sub {
		color: var(--faint);
		font-size: 11px;
	}
	.empty {
		color: var(--faint);
	}

	@media (max-width: 700px) {
		.content {
			padding: 18px 16px 24px;
		}
		.search-field {
			margin-left: 0;
		}
	}
</style>
