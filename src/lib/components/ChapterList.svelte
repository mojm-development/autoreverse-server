<script lang="ts">
	import ListRow from './ListRow.svelte';

	let {
		chapters,
		currentPosition,
		isPlayingThis,
		onSelect
	}: {
		chapters: { title: string; start: number; end: number }[];
		currentPosition: number;
		isPlayingThis: boolean;
		/** Required rather than optional: a chapter list nobody can jump from is
		 * a decorative table, and omitting it should be a type error, not a
		 * silently dead list. */
		onSelect: (start: number) => void;
	} = $props();

	const currentChapter = $derived(
		chapters.find((c) => currentPosition >= c.start && currentPosition < c.end) ??
			chapters[chapters.length - 1] ??
			null
	);

	function formatHMS(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${h}:${pad(m)}:${pad(s)}`;
	}
</script>

<div class="table" role="table" aria-label="Kapitel">
	{#each chapters as c, i (c.title + c.start)}
		{@const playing = isPlayingThis && c === currentChapter}
		{@const heard = !playing && currentPosition >= c.end}
		<ListRow ariaCurrent={playing} label="{c.title} abspielen" onclick={() => onSelect(c.start)}>
			<span class="index mono">{i + 1}</span>
			<span class="title">{c.title}</span>
			<span class="cell state">{playing ? 'läuft' : heard ? 'gehört' : ''}</span>
			<span class="cell mono">{formatHMS(c.start)}</span>
		</ListRow>
	{/each}
</div>

<style>
	.table {
		display: flex;
		flex-direction: column;
	}
	.index {
		width: 20px;
		flex: none;
		color: var(--faint);
	}
	.title {
		flex: 2;
		min-width: 0;
	}
	.cell {
		flex: 1;
		color: var(--dim);
		font-size: 12px;
	}
	.state {
		color: var(--a);
		font-weight: 500;
	}
	:global([role='row'][aria-current='true']) {
		background: color-mix(in oklab, var(--a) 12%, transparent);
	}
</style>
