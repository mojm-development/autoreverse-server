<script lang="ts">
	import type { Snippet } from 'svelte';
	let { href, ariaCurrent, children }: { href?: string; ariaCurrent?: boolean; children: Snippet } =
		$props();
</script>

{#if href}
	<a {href} role="row" aria-current={ariaCurrent} class="row">
		{@render children()}
	</a>
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
</style>
