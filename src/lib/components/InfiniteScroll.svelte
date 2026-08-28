<script lang="ts">
	let {
		onLoadMore,
		done = false,
		loading = false,
		label = 'Weitere Einträge werden geladen'
	}: {
		onLoadMore: () => void;
		done?: boolean;
		loading?: boolean;
		label?: string;
	} = $props();

	let sentinel = $state<HTMLElement | null>(null);

	$effect(() => {
		const element = sentinel;
		const busy = loading;
		if (!element || done || busy) return;
		const observer = new IntersectionObserver(
			(entries) => {
				if (entries.some((entry) => entry.isIntersecting)) onLoadMore();
			},
			{ rootMargin: '800px 0px' }
		);
		observer.observe(element);
		return () => observer.disconnect();
	});
</script>

{#if !done}
	<div bind:this={sentinel} class="sentinel"></div>
{/if}
<p class="status" role="status" aria-live="polite">
	{#if loading}{label}…{/if}
</p>

<style>
	.sentinel {
		height: 1px;
	}
	.status {
		min-height: 18px;
		margin: 14px 0 0;
		color: var(--faint);
		font-size: 11.5px;
	}
</style>
