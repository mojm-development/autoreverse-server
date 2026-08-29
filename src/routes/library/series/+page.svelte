<script lang="ts">
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
</script>

<PageTitle title="Serien" />

<div class="content" style="--a: var(--book)">
	<h1>Serien <span class="count mono">{data.series.length}</span></h1>
	<div class="grid">
		{#each data.series as s (s.series)}
			<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
			<a class="tile" href="/library/books?series={encodeURIComponent(s.series ?? '')}">
				<span class="name">{s.series}</span>
				<span class="count-sub mono">{s.count} Bände</span>
			</a>
		{/each}
	</div>
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
	.grid {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 12px;
	}
	.tile {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 14px;
		border-radius: var(--radius-lg);
		background: var(--panel);
		color: inherit;
	}
	.name {
		font: 500 13px var(--font-sans);
	}
	.count-sub {
		color: var(--faint);
		font-size: 11px;
	}
</style>
