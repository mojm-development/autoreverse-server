<script lang="ts">
	let {
		current,
		options
	}: {
		current: string;
		options: { key: string; label: string; href: string }[];
	} = $props();
</script>

<div class="sort-toggle" role="group" aria-label="Sortierung">
	{#each options as option (option.key)}
		<a href={option.href} aria-current={current === option.key}>{option.label}</a>
	{/each}
</div>

<style>
	.sort-toggle {
		display: flex;
		gap: 4px;
	}
	.sort-toggle a {
		display: inline-flex;
		align-items: center;
		height: 28px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		font: 500 11.5px var(--font-sans);
		color: var(--dim);
		border: 1px solid var(--line);
	}
	.sort-toggle a[aria-current='true'] {
		background: var(--panel-hi);
		color: var(--text);
	}
</style>
