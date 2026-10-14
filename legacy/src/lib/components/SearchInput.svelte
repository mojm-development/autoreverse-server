<script lang="ts">
	import Icon from './Icon.svelte';
	let {
		value,
		placeholder,
		resultMeta,
		oninput,
		variant = 'full'
	}: {
		value: string;
		placeholder: string;
		resultMeta?: string;
		oninput: (v: string) => void;
		variant?: 'compact' | 'full';
	} = $props();
</script>

<div class="search-input {variant}">
	<Icon name="search" />
	<input type="search" {placeholder} {value} oninput={(e) => oninput(e.currentTarget.value)} />
	{#if resultMeta}
		<span class="meta mono">{resultMeta}</span>
	{/if}
</div>

<style>
	.search-input {
		display: flex;
		align-items: center;
		gap: 8px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		padding: 0 10px;
	}
	.search-input.compact {
		height: 32px;
	}
	.search-input.full {
		height: 40px;
	}
	input {
		flex: 1;
		border: none;
		background: transparent;
		color: var(--text);
		font: 400 13px var(--font-sans);
		outline: none;
	}
	input::-webkit-search-cancel-button {
		display: none;
	}
	.meta {
		color: var(--faint);
		font-size: 11px;
		white-space: nowrap;
	}
</style>
