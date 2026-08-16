<script lang="ts">
	import { resolve } from '$app/paths';
	import Sidebar from '$lib/components/Sidebar.svelte';
	let { data, children } = $props();
	const items = [
		{ href: '/settings/libraries', label: 'Bibliotheken', admin: true },
		{ href: '/settings/scan', label: 'Scan', admin: true },
		{ href: '/settings/users', label: 'Nutzer', admin: true },
		{ href: '/settings/playback', label: 'Wiedergabe', admin: false },
		{ href: '/settings/podcasts', label: 'Podcast-Abos', admin: true },
		{ href: '/settings/security', label: 'Sicherheit', admin: false },
		{ href: '/settings/about', label: 'Über Autoreverse', admin: false }
	] as const;
</script>

<div class="shell">
	{#if data.user && data.counts}
		<!-- `data.counts` is only optional in this route's generated PageData type because this
			 layout's own +layout.server.ts (needed for the admin-only redirect) flattens the root
			 layout's `{user, counts}` discriminated union via SvelteKit's Omit-based data merging —
			 at runtime the root load always returns both together or neither. Checking both here
			 (rather than asserting `data.counts!`) resolves the same type error without trusting an
			 assumption, and fails closed if that invariant is ever broken by a future root-layout edit. -->
		<Sidebar accent="book" activeHref="/settings" user={data.user} counts={data.counts} />
	{/if}
	<div class="settings" style="--a: var(--book)">
		<nav class="subnav">
			{#each items as it (it.href)}
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
		grid-template-columns: 236px 1fr;
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
		padding: 24px 32px;
	}
</style>
