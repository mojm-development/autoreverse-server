<script lang="ts">
	import PodcastSearch from '$lib/components/PodcastSearch.svelte';
	import PodcastRail from '$lib/components/PodcastRail.svelte';
	import { invalidateAll } from '$app/navigation';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
</script>

<div class="layout" style="--a: var(--podcast)">
	<PodcastRail podcasts={data.podcasts} />
	<div class="content">
		{#if data.podcasts.length > 0}
			<p class="empty">Wähle einen Podcast aus der Liste.</p>
		{:else}
			<p class="empty">Noch keine Podcast-Abos.</p>
		{/if}
		{#if data.user?.isAdmin}
			<section class="add-feed">
				<h2>Podcast abonnieren</h2>
				<PodcastSearch initialQuery={data.query} onSubscribed={() => invalidateAll()} />
			</section>
		{/if}
	</div>
</div>

<style>
	.layout {
		display: flex;
		min-height: 100%;
	}
	.add-feed {
		margin-top: 28px;
		padding-top: 20px;
		border-top: 1px solid var(--line);
		max-width: 1100px;
	}
	.add-feed h2 {
		font: 600 15px var(--font-sans);
		margin-bottom: 12px;
	}
	.content {
		flex: 1;
		min-width: 0;
		padding: 24px 32px;
	}
	.empty {
		color: var(--faint);
	}
</style>
