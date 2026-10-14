<script lang="ts">
	import { resolve } from '$app/paths';
	import ScanStatusCard from '$lib/components/ScanStatusCard.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData, ActionData } from './$types';
	let { data, form }: { data: PageData; form: ActionData } = $props();
</script>

<PageTitle title="Bibliotheken" />

<h1>Bibliotheken</h1>

<form method="POST" action="?/savePaths" class="path-form">
	<label>
		<span class="eyebrow">Bücher-Pfad</span>
		<input name="booksDir" value={data.paths.booksDir ?? ''} placeholder="/library/books" />
	</label>
	<label>
		<span class="eyebrow">Musik-Pfad</span>
		<input name="musicDir" value={data.paths.musicDir ?? ''} placeholder="/library/music" />
	</label>
	<button type="submit" class="primary">Speichern</button>
	{#if form?.error}<p class="error">{form.error}</p>{/if}
</form>

<ScanStatusCard />

{#if data.missing > 0}
	<div class="card warning">
		<span>{data.missing} Titel fehlen</span>
		<a href={resolve('/library/albums?missing=true')}>Anzeigen</a>
		<form method="POST" action="?/cleanupMissing">
			<button type="submit">Aufräumen</button>
		</form>
	</div>
{/if}

<h2>Nutzer</h2>
<a href={resolve('/settings/users')}>Nutzer verwalten</a>

<style>
	h1 {
		font: 600 20px var(--font-sans);
	}
	.path-form {
		display: flex;
		flex-direction: column;
		gap: 10px;
		max-width: 420px;
		margin-bottom: 20px;
	}
	.path-form input {
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
	}
	.primary {
		align-self: flex-start;
		height: 32px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		background: var(--a);
		color: var(--bg);
		border: none;
	}
	.error {
		color: var(--music);
		font-size: 12px;
	}
	.card.warning {
		margin-top: 16px;
		padding: 14px;
		border-radius: var(--radius-lg);
		background: color-mix(in oklab, var(--music) 10%, transparent);
		border: 1px solid color-mix(in oklab, var(--music) 30%, transparent);
		display: flex;
		align-items: center;
		gap: 14px;
	}
</style>
