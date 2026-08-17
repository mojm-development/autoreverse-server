<script lang="ts">
	import { resolve } from '$app/paths';
	import PodcastSearch from '$lib/components/PodcastSearch.svelte';
</script>

<h1>Podcast-Abos</h1>
<PodcastSearch onSubscribed={() => {}} />

<p class="hint">
	Aktuelle Abos und deren Verwaltung (aktualisieren/kündigen) unter
	<a href={resolve('/library/podcasts')}>Podcasts</a>.
</p>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
	}
	.hint {
		color: var(--faint);
		font-size: 12px;
		margin-top: 16px;
	}
</style>
