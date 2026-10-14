<script lang="ts">
	let { view, gridHref, listHref }: { view: 'grid' | 'list'; gridHref: string; listHref: string } =
		$props();
</script>

<div class="view-toggle" role="group" aria-label="Ansicht">
	<a href={gridHref} aria-current={view === 'grid'}>Raster</a>
	<a href={listHref} aria-current={view === 'list'}>Liste</a>
</div>

<style>
	.view-toggle {
		display: flex;
		gap: 4px;
	}
	.view-toggle a {
		display: inline-flex;
		align-items: center;
		height: 28px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		font: 500 11.5px var(--font-sans);
		color: var(--dim);
		border: 1px solid var(--line);
	}
	.view-toggle a[aria-current='true'] {
		background: var(--panel-hi);
		color: var(--text);
	}
</style>
