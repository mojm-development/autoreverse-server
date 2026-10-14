<script lang="ts">
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	let draggedId = $state<number | null>(null);

	function formatDuration(seconds: number | null): string {
		if (seconds === null) return '';
		const m = Math.floor(seconds / 60);
		const s = Math.floor(seconds % 60);
		return `${m}:${String(s).padStart(2, '0')}`;
	}

	async function onDrop(targetPosition: number) {
		if (draggedId === null) return;
		await fetch(`/playlists/${data.playlist.id}/entries/${draggedId}`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position: targetPosition })
		});
		draggedId = null;
		location.reload();
	}
</script>

<PageTitle title={data.playlist.name} />

<div class="content" style="--a: var(--music)">
	<header><h1>{data.playlist.name}</h1></header>

	<div class="table" role="table" aria-label="Titel">
		{#each data.entries as entry, i (entry.id)}
			<div
				class="row"
				role="row"
				tabindex="0"
				draggable="true"
				ondragstart={() => (draggedId = entry.id)}
				ondragover={(e) => e.preventDefault()}
				ondrop={() => onDrop(i + 1)}
			>
				<span class="index mono">{i + 1}</span>
				<span class="title">{entry.title}</span>
				<span class="cell">{entry.subtitle}</span>
				<span class="cell mono">{formatDuration(entry.duration)}</span>
			</div>
		{/each}
	</div>
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	header {
		margin-bottom: 20px;
	}
	.table {
		display: flex;
		flex-direction: column;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 44px;
		padding: 0 12px;
		border-radius: var(--radius-sm);
		cursor: grab;
	}
	.row:hover {
		background: var(--panel);
	}
	.index {
		width: 24px;
		flex: none;
		color: var(--faint);
	}
	.title {
		flex: 2;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cell {
		flex: 1;
		color: var(--dim);
		font-size: 12px;
	}
</style>
