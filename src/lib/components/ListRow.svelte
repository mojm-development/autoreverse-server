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
		/** Makes the whole row activatable. Requires `label` for its accessible name. */
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
	<!-- The row is activatable, but rows also carry their own controls (a
		favourite button, a download button). Wrapping the children in a <button>
		would be invalid HTML — a button may not contain interactive content, and
		the parser closes the outer one at the inner one, collapsing the layout.
		So the activator is a real button stretched across the row instead. It
		sits above the content, which keeps clicks on text working; a control
		that needs its own click raises itself past it with class="above". -->
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
	/* Opt-in escape hatch for a control that must stay clickable through the
	   activator. :global because the children come from the calling page. */
	.clickable :global(.above) {
		position: relative;
		z-index: 2;
	}
</style>
