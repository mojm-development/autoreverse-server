<script lang="ts">
	import { getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import ChapterList from '$lib/components/ChapterList.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let tab = $state<'chapters' | 'bookmarks' | 'series'>('chapters');

	const totalDuration = $derived(data.tracks.reduce((sum, t) => sum + t.duration, 0));
	const isPlayingThis = $derived(player.current?.itemId === data.book.id);
	const currentPosition = $derived(
		isPlayingThis ? player.current!.position : (data.progress?.position ?? 0)
	);
	const currentChapter = $derived(
		data.chapters.find((c) => currentPosition >= c.start && currentPosition < c.end) ??
			data.chapters[data.chapters.length - 1] ??
			null
	);
	const remaining = $derived(Math.max(0, totalDuration - currentPosition));
	const percent = $derived(
		totalDuration > 0 ? Math.round((currentPosition / totalDuration) * 100) : 0
	);

	function formatHMS(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${h}:${pad(m)}:${pad(s)}`;
	}
	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
	}

	function playFrom(position: number) {
		void player.playFrom(data.book.id, position);
	}
	async function restart() {
		await player.play(data.book.id);
		player.seek(0);
	}
	async function addBookmarkHere() {
		const position = currentPosition;
		await fetch('/bookmarks', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ item_id: data.book.id, position, title: formatHMS(position) })
		});
	}
</script>

<div class="content" style="--a: var(--book)">
	<div class="hero">
		<div class="cover">
			{#if data.book.coverPath}
				<img src="/items/{data.book.id}/cover" alt="" />
			{/if}
		</div>
		<div class="meta">
			<div class="eyebrow-row">
				<span class="eyebrow">Hörbuch</span>
				{#if data.book.series}
					<span class="pill">Serie · {data.seriesBooks.length} Bände</span>
				{/if}
			</div>
			<h1>{data.book.title}</h1>
			<p class="subline">
				{data.book.author ?? ''}{#if data.book.narrator}
					· gelesen von {data.book.narrator}{/if} ·
				{data.chapters.length} Kapitel · {formatDuration(totalDuration)}
			</p>
			<div class="actions">
				<button class="primary" onclick={() => player.play(data.book.id)}>
					<Icon name="play" />
					{data.progress ? `Weiter ab ${formatHMS(data.progress.position)}` : 'Abspielen'}
				</button>
				<button class="outline" onclick={restart}><Icon name="previous" /> Von vorn</button>
				<button class="outline" onclick={addBookmarkHere}
					><Icon name="bookmark" /> Lesezeichen setzen</button
				>
			</div>
		</div>
	</div>

	<div class="progress-row">
		<span class="chapter-name">{currentChapter?.title ?? ''}</span>
		<span class="progress-meta mono">{formatDuration(remaining)} übrig · {percent} %</span>
	</div>
	<div class="scrubber">
		{#each data.chapters as c (c.title + c.start)}
			<span class="tick" style="left: {totalDuration > 0 ? (c.start / totalDuration) * 100 : 0}%"
			></span>
		{/each}
		<div class="fill" style="width: {percent}%"></div>
	</div>

	<div class="tabs">
		<button class="tab" class:active={tab === 'chapters'} onclick={() => (tab = 'chapters')}
			>Kapitel</button
		>
		<button class="tab" class:active={tab === 'bookmarks'} onclick={() => (tab = 'bookmarks')}>
			Lesezeichen · {data.bookmarks.length}
		</button>
		{#if data.book.series}
			<button class="tab" class:active={tab === 'series'} onclick={() => (tab = 'series')}>
				Serie · {data.seriesBooks.length} Bände
			</button>
		{/if}
		<span class="tab-trailing mono">{data.chapters.length} Kapitel</span>
	</div>

	{#if tab === 'chapters'}
		<ChapterList chapters={data.chapters} {currentPosition} {isPlayingThis} onSelect={playFrom} />
	{:else if tab === 'bookmarks'}
		<div class="table" role="table" aria-label="Lesezeichen">
			{#each data.bookmarks as b (b.id)}
				<ListRow label="Lesezeichen {b.title} abspielen" onclick={() => playFrom(b.position)}>
					<span class="title">{b.title}</span>
					<span class="cell mono">{formatHMS(b.position)}</span>
				</ListRow>
			{/each}
		</div>
	{:else if tab === 'series'}
		<div class="table" role="table" aria-label="Serie">
			{#each data.seriesBooks as sibling (sibling.id)}
				<ListRow href="/library/books/{sibling.id}">
					<span class="title">{sibling.title}</span>
				</ListRow>
			{/each}
		</div>
	{/if}
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	.hero {
		display: flex;
		gap: 24px;
		margin-bottom: 28px;
	}
	.cover {
		width: 186px;
		height: 186px;
		flex: none;
		border-radius: var(--radius-lg);
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
		overflow: hidden;
	}
	.cover img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.meta {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 6px;
	}
	.eyebrow-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	h1 {
		font: 600 26px/1.2 var(--font-sans);
		margin: 0;
	}
	.subline {
		color: var(--dim);
		font-size: 13px;
		margin: 0;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-top: 14px;
	}
	.primary,
	.outline {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		height: 36px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		font: 500 13px var(--font-sans);
		border: none;
	}
	.primary {
		background: var(--a);
		color: var(--bg);
	}
	.outline {
		background: transparent;
		border: 1px solid var(--line-strong);
		color: var(--text);
	}
	.progress-row {
		display: flex;
		justify-content: space-between;
		margin-bottom: 6px;
	}
	.chapter-name {
		font: 500 12.5px var(--font-sans);
	}
	.progress-meta {
		color: var(--faint);
		font-size: 11px;
	}
	.scrubber {
		position: relative;
		height: 6px;
		border-radius: 99px;
		background: var(--track);
		margin-bottom: 22px;
	}
	.fill {
		position: absolute;
		inset: 0 auto 0 0;
		background: var(--a);
		border-radius: 99px;
	}
	.tick {
		position: absolute;
		top: -2px;
		width: 1px;
		height: 10px;
		background: var(--track-tick);
	}
	.tabs {
		display: flex;
		align-items: center;
		gap: 18px;
		border-bottom: 1px solid var(--line);
		margin-bottom: 10px;
		padding-bottom: 8px;
	}
	.tab {
		background: none;
		border: none;
		color: var(--dim);
		font: 500 12.5px var(--font-sans);
		padding: 0;
	}
	.tab.active {
		color: var(--text);
	}
	.tab-trailing {
		margin-left: auto;
		color: var(--faint);
		font-size: 11px;
	}
	.table {
		display: flex;
		flex-direction: column;
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
</style>
