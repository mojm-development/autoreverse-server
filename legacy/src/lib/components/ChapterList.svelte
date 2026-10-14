<script lang="ts">
	import ListRow from './ListRow.svelte';
	import { chapterLabels } from '$lib/chapterTitles';

	let {
		chapters,
		currentPosition,
		isPlayingThis,
		onSelect,
		itemTitle = null
	}: {
		chapters: { title: string; start: number; end: number }[];
		currentPosition: number;
		isPlayingThis: boolean;
		onSelect: (start: number) => void;
		/** Lets the list drop a book title that every chapter repeats. */
		itemTitle?: string | null;
	} = $props();

	const labels = $derived(
		chapterLabels(
			chapters.map((c) => c.title),
			itemTitle
		)
	);

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
			<span class="title">{labels[i] ?? c.title}</span>
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
		width: 22px;
		flex: none;
		text-align: right;
		color: var(--faint);
	}
	/* One line, ellipsed: the rows have a fixed height, so a wrapping title used to
	   run over the chapter below it. */
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
		white-space: nowrap;
	}
	.state {
		flex: none;
		width: 54px;
		color: var(--a);
		font-weight: 500;
	}
	:global([role='row'][aria-current='true']) {
		background: color-mix(in oklab, var(--a) 12%, transparent);
	}
</style>
