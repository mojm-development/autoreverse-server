<script lang="ts">
	import '../app.css';
	import { setContext } from 'svelte';
	import favicon from '$lib/assets/favicon.svg';
	import { createPlayerStore, PLAYER_CONTEXT_KEY } from '$lib/player.svelte';
	import MiniPlayerBar from '$lib/components/MiniPlayerBar.svelte';
	import IconSprite from '$lib/components/IconSprite.svelte';
	let { children, data } = $props();

	const player = createPlayerStore();
	setContext(PLAYER_CONTEXT_KEY, player);

	// Flush progress when the tab closes — sendBeacon doesn't need a response.
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
	<link rel="icon" href={favicon} />
</svelte:head>

<IconSprite />

{@render children()}
{#if data.user}
	<MiniPlayerBar />
{/if}
