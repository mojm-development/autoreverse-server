<script lang="ts">
	import { resolve } from '$app/paths';
	import { enhance } from '$app/forms';
	import PodcastSearch from '$lib/components/PodcastSearch.svelte';
	import type { PageData, ActionData } from './$types';
	let { data, form }: { data: PageData; form: ActionData } = $props();
</script>

<h1>Podcast-Abos</h1>

<section class="keep">
	<h2>Folgen vorhalten</h2>
	<form method="POST" action="?/keep" use:enhance>
		<label for="keep">Neueste Folgen je Abo automatisch laden</label>
		<input
			id="keep"
			name="keep"
			type="number"
			min="0"
			max={data.keepMax}
			value={data.keepDefault}
		/>
		<button type="submit" class="primary">Speichern</button>
	</form>
	<p class="hint">
		0 schaltet das Vorhalten ab. Ältere Downloads über dieser Zahl werden beim Aktualisieren wieder
		gelöscht; ein einzelnes Abo kann den Wert auf seiner Seite überschreiben.
		{#if data.refreshHours > 0}
			Der Server aktualisiert alle Feeds alle {data.refreshHours} Stunden von selbst.
		{:else}
			Die automatische Aktualisierung ist abgeschaltet (AUTOREVERSE_PODCAST_REFRESH_HOURS=0).
		{/if}
	</p>
	{#if form?.error}
		<p class="error" role="alert">{form.error}</p>
	{/if}
</section>

<h2>Abonnieren</h2>
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
	h2 {
		font: 600 13px var(--font-sans);
		color: var(--dim);
		margin: 0 0 10px;
	}
	.keep {
		margin-bottom: 28px;
		padding-bottom: 20px;
		border-bottom: 1px solid var(--line);
	}
	.keep form {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.keep input {
		width: 80px;
	}
	.error {
		color: var(--music);
		font-size: 12.5px;
		margin: 8px 0 0;
	}
	.hint {
		color: var(--faint);
		font-size: 12px;
		margin-top: 16px;
	}
</style>
