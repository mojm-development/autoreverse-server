<script lang="ts">
	import { onMount } from 'svelte';
	import { getTheme, toggleTheme } from '$lib/theme';
	import Icon from './Icon.svelte';

	let effective = $state<'dark' | 'light'>('dark');

	onMount(() => {
		effective =
			getTheme() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
	});

	function handleClick() {
		effective = toggleTheme();
	}
</script>

<button
	class="theme-toggle"
	onclick={handleClick}
	aria-label="Thema wechseln"
	title={effective === 'dark' ? 'Zu hellem Thema wechseln' : 'Zu dunklem Thema wechseln'}
>
	<Icon name={effective === 'dark' ? 'moon' : 'sun'} />
</button>

<style>
	/* An icon rather than the word: three equal buttons sit in the account box. */
	.theme-toggle {
		flex: 1;
		height: 30px;
		padding: 0;
		display: grid;
		place-items: center;
		border-radius: var(--radius-pill);
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
		font-size: 15px;
	}
	.theme-toggle:hover {
		color: var(--text);
		background: var(--panel-hi);
		border-color: var(--line-strong);
	}
</style>
