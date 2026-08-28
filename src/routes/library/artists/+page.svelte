<script lang="ts">
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
</script>

<div class="content" style="--a: var(--music)">
	<h1>Interpreten <span class="count mono">{data.artists.length}</span></h1>
	<div class="grid">
		{#each data.artists as a (a.name)}
			<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
			<a class="tile" href="/library/albums?artist={encodeURIComponent(a.name ?? '')}">
				<span class="name">{a.name}</span>
				<span class="count-sub mono">{a.albumCount} Alben</span>
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
