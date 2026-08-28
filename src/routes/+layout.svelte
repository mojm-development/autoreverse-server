<script lang="ts">
	import '../app.css';
	import { setContext, untrack } from 'svelte';

	import { createPlayerStore, PLAYER_CONTEXT_KEY } from '$lib/player.svelte';
	import MiniPlayerBar from '$lib/components/MiniPlayerBar.svelte';
	import IconSprite from '$lib/components/IconSprite.svelte';
	let { children, data } = $props();

	const player = createPlayerStore(untrack(() => data.preferences));
	setContext(PLAYER_CONTEXT_KEY, player);

	if (typeof window !== 'undefined') {
		window.addEventListener('beforeunload', () => {
			const current = player.current;
			if (current) {
				navigator.sendBeacon(
					`/progress/${current.itemId}`,
					new Blob([JSON.stringify({ position: current.position, finished: false })], {
						type: 'application/json'
					})
				);
			}
		});
	}
</script>

<svelte:head>
	<link rel="icon" href="/favicon.png" sizes="any" />
	<link rel="apple-touch-icon" href="/apple-touch-icon.png" />
</svelte:head>

<IconSprite />

{@render children()}
{#if data.user}
	<MiniPlayerBar />
{/if}
