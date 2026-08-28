<script lang="ts">
	import { resolve } from '$app/paths';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { SETTINGS_SECTIONS } from '$lib/settingsSections';
	let { data, children } = $props();
</script>

<div class="shell">
	{#if data.user && data.counts}
		<Sidebar accent="book" activeHref="/settings" user={data.user} counts={data.counts} />
	{/if}
	<div class="settings" style="--a: var(--book)">
		<nav class="subnav">
			<a href={resolve('/settings')}>Übersicht</a>
			{#each SETTINGS_SECTIONS as it (it.href)}
				{#if !it.admin || data.user?.isAdmin}
					<a href={resolve(it.href)}>{it.label}</a>
				{/if}
			{/each}
		</nav>
		<div class="content">{@render children()}</div>
	</div>
</div>

<style>
	.shell {
		display: grid;
		grid-template-columns: var(--sidebar-width) 1fr;
		min-height: 100vh;
	}
	.settings {
		display: grid;
		grid-template-columns: 216px 1fr;
	}
	.subnav {
		display: flex;
		flex-direction: column;
		padding: 20px 12px;
		border-right: 1px solid var(--line);
		gap: 2px;
	}
	.subnav a {
		padding: 8px 10px;
		border-radius: 7px;
		color: var(--dim);
		font: 400 13px var(--font-sans);
	}
	.subnav a:hover {
		background: var(--panel);
		color: var(--text);
	}
	.content {
		padding: 24px 32px var(--player-bar-height);
	}
</style>
