<script lang="ts">
	let {
		count,
		total,
		allMatching = false,
		onSelectAll,
		onClear,
		onEdit
	}: {
		count: number;
		/** How many the current filter matches in total, across pages. */
		total?: number;
		/** True when the whole filter is selected, not just the loaded page. */
		allMatching?: boolean;
		onSelectAll?: () => void;
		onClear: () => void;
		onEdit: () => void;
	} = $props();
</script>

{#if count > 0}
	<div class="bar" role="status">
		<span class="count">
			{allMatching && total ? `Alle ${total}` : count} ausgewählt
		</span>
		{#if onSelectAll && total && total > count && !allMatching}
			<button class="link" onclick={onSelectAll}>Alle {total} auswählen</button>
		{/if}
		<button class="link" onclick={onClear}>Abwählen</button>
		<button class="primary" onclick={onEdit}>Bearbeiten</button>
	</div>
{/if}

<style>
	/* Sits above the mini player, which owns the very bottom of the screen. */
	.bar {
		position: fixed;
		left: 50%;
		bottom: calc(var(--player-bar-height) + 16px);
		transform: translateX(-50%);
		z-index: 30;
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 10px 12px 10px 18px;
		border-radius: var(--radius-pill);
		border: 1px solid var(--line-strong);
		background: var(--sidebar);
		box-shadow: 0 18px 40px -20px rgb(0 0 0 / 0.85);
	}
	.count {
		font: 500 12.5px var(--font-sans);
	}
	.link {
		height: 26px;
		padding: 0 8px;
		border: none;
		background: transparent;
		color: var(--dim);
		font: 500 12px var(--font-sans);
	}
	.link:hover {
		color: var(--text);
	}
	.primary {
		height: 30px;
		padding: 0 16px;
		border: none;
		border-radius: var(--radius-pill);
		background: var(--a);
		color: var(--bg);
		font: 500 12.5px var(--font-sans);
	}

	@media (max-width: 700px) {
		.bar {
			bottom: calc(var(--player-bar-height) + var(--mobile-nav-height) + 12px);
		}
	}
</style>
