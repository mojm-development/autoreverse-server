<script lang="ts">
	import { resolve } from '$app/paths';
	import { SETTINGS_SECTIONS } from '$lib/settingsSections';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
	const sections = $derived(SETTINGS_SECTIONS.filter((s) => !s.admin || data.user?.isAdmin));
</script>

<h1>Einstellungen</h1>
<p class="lede">Alles, was sich an diesem Server einstellen lässt — nach Bereichen sortiert.</p>

<div class="cards" data-testid="settings-overview">
	{#each sections as section (section.href)}
		<a class="card" href={resolve(section.href)}>
			<span class="label">{section.label}</span>
			<span class="desc">{section.description}</span>
			{#if section.admin}<span class="tag mono">nur Verwalter</span>{/if}
		</a>
	{/each}
</div>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 6px;
	}
	.lede {
		color: var(--dim);
		font: 400 13px/1.5 var(--font-sans);
		margin-bottom: 20px;
		max-width: 52ch;
	}
	.cards {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: 10px;
	}
	.card {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 5px;
		padding: 14px;
		border: 1px solid var(--line);
		border-radius: var(--radius-md);
		background: var(--panel);
		color: var(--text);
	}
	.card:hover {
		border-color: var(--a);
	}
	.label {
		font: 500 14px/1.2 var(--font-sans);
	}
	.desc {
		color: var(--dim);
		font: 400 12.5px/1.45 var(--font-sans);
	}
	.tag {
		margin-top: 3px;
		font-size: 10px;
		color: var(--faint);
	}
</style>
