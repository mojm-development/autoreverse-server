<script lang="ts">
	import Icon from '$lib/components/Icon.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function formatDuration(seconds: number): string {
		return (seconds / 3600).toFixed(1);
	}
</script>

<div class="content" style="--a: var(--music)">
	<header><h1>Playlists</h1></header>

	<!-- ponytail: "Automatisch"/"Geteilt" have no backend concept at all (one
	     playlist kind, user-owned, no sharing/smart rules) — omitted entirely
	     rather than rendered as dead UI, matching the genre/narrator/format
	     omission precedent from Tasks 33-35. -->
	<div class="pills">
		<span class="pill active">Eigene</span>
	</div>

	<div class="grid">
		{#each data.playlists as playlist (playlist.id)}
			<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
			<a class="tile" href="/library/playlists/{playlist.id}">
				<div class="collage"><Icon name="playlist" /></div>
				<span class="title">{playlist.name}</span>
				<span class="subtitle"
					>{playlist.entryCount} Titel · {formatDuration(playlist.duration)} h</span
				>
			</a>
		{/each}
	</div>
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	header {
		margin-bottom: 14px;
	}
	.pills {
		display: flex;
		gap: 6px;
		margin-bottom: 20px;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 18px;
	}
	.tile {
		color: inherit;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.collage {
		aspect-ratio: 1;
		border-radius: var(--radius-lg);
		background: var(--tile);
		display: grid;
		place-items: center;
		color: var(--faint);
	}
	.title {
		font: 500 13px var(--font-sans);
	}
	.subtitle {
		font: 400 11px var(--font-sans);
		color: var(--dim);
	}
</style>
