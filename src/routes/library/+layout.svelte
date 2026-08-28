<script lang="ts">
	import { page } from '$app/state';
	import Sidebar from '$lib/components/Sidebar.svelte';
	let { data, children } = $props();

	const ACCENTS: Record<string, 'book' | 'music' | 'podcast'> = {
		'/library': 'book',
		'/library/search': 'music',
		'/library/albums': 'music',
		'/library/books': 'book',
		'/library/podcasts': 'podcast',
		'/library/playlists': 'music',
		'/library/favorites': 'music',
		'/library/artists': 'music',
		'/library/series': 'book',
		'/library/bookmarks': 'book'
	};

	function accentFor(pathname: string): 'book' | 'music' | 'podcast' {
		const candidates = Object.keys(ACCENTS).filter(
			(p) => pathname === p || pathname.startsWith(`${p}/`)
		);
		const longest = candidates.sort((a, b) => b.length - a.length)[0];
		return ACCENTS[longest ?? '/library'];
	}

	const accent = $derived(accentFor(page.url.pathname));
</script>

{#if data.user}
	<div class="shell">
		<Sidebar {accent} activeHref={page.url.pathname} user={data.user} counts={data.counts} />
		<main>{@render children()}</main>
	</div>
{:else}
	{@render children()}
{/if}

<style>
	.shell {
		display: grid;
		grid-template-columns: var(--sidebar-width) 1fr;
		min-height: 100vh;
	}
	main {
		min-width: 0;
		/* Unconditional: the bar is fixed, so on a short page this padding sits
		   inside the shell's 100vh and costs nothing, and on a long one it is
		   what keeps the last row clear of the bar. */
		padding-bottom: var(--player-bar-height);
	}
	@media (max-width: 700px) {
		main {
			padding-bottom: calc(var(--player-bar-height) + var(--mobile-nav-height));
		}
	}
</style>
