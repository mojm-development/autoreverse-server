<script lang="ts">
	import { onMount } from 'svelte';
	import { getTheme, toggleTheme } from '$lib/theme';

	let effective = $state<'dark' | 'light'>('dark');

	onMount(() => {
		effective =
			getTheme() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
	});

	function handleClick() {
		effective = toggleTheme();
	}
</script>

<button class="theme-toggle" onclick={handleClick} aria-label="Thema wechseln">
	{effective === 'dark' ? 'Dunkel' : 'Hell'}
</button>

<style>
	.theme-toggle {
		height: 26px;
		padding: 0 10px;
		border-radius: var(--radius-pill);
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
		font: 500 11px var(--font-sans);
	}
</style>
