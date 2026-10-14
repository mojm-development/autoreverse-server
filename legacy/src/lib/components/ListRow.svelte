<script lang="ts">
	import type { Snippet } from 'svelte';
	let {
		href,
		onclick,
		label,
		ariaCurrent,
		children
	}: {
		href?: string;
		onclick?: () => void;
		label?: string;
		ariaCurrent?: boolean;
		children: Snippet;
	} = $props();
</script>

{#if href}
	<a {href} role="row" aria-current={ariaCurrent} class="row">
		{@render children()}
	</a>
{:else if onclick}
	<div role="row" aria-current={ariaCurrent} class="row clickable">
		<button type="button" class="activator bare" {onclick} aria-label={label}></button>
		{@render children()}
	</div>
{:else}
	<div role="row" aria-current={ariaCurrent} class="row">
		{@render children()}
	</div>
{/if}

<style>
	.row {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 52px;
		padding: 0 12px;
		border-radius: var(--radius-sm);
		color: inherit;
	}
	.row:hover {
		background: var(--panel);
	}
	.clickable {
		position: relative;
	}
	.activator {
		position: absolute;
		inset: 0;
		z-index: 1;
		width: 100%;
		border-radius: inherit;
	}
	.activator:focus-visible {
		outline: 2px solid var(--a, var(--book));
		outline-offset: -2px;
	}
	.clickable :global(.above) {
		position: relative;
		z-index: 2;
	}
</style>
