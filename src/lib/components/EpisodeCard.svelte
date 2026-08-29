<script lang="ts">
	import Icon from './Icon.svelte';
	import { humanDuration } from '$lib/dates';

	let {
		title,
		date = '',
		duration = 0,
		downloaded = false,
		position = 0,
		finished = false,
		coverStyle = '',
		index,
		show,
		onPlay,
		onDownload,
		downloading = false
	}: {
		title: string;
		date?: string;
		duration?: number;
		downloaded?: boolean;
		position?: number;
		finished?: boolean;
		/** Background shorthand for the artwork tile; omitted, the tile is dropped. */
		coverStyle?: string;
		/** Episode number, shown when there is no artwork to show instead. */
		index?: number;
		show?: { title: string; href: string };
		onPlay: () => void;
		onDownload?: () => void;
		downloading?: boolean;
	} = $props();

	const started = $derived(!finished && position > 0);
	const percent = $derived(duration > 0 ? Math.min(100, (position / duration) * 100) : 0);
	const left = $derived(humanDuration(Math.max(0, duration - position)));
</script>

<li class="episode" class:finished>
	{#if coverStyle}
		<span class="cover" style={coverStyle}></span>
	{:else if index !== undefined}
		<span class="index mono" aria-hidden="true">{index}</span>
	{/if}
	<div class="body">
		<h3 class="title">{title}</h3>
		<p class="meta">
			{#if show}
				<a class="show" href={show.href}>{show.title}</a>
				<span class="dot">·</span>
			{/if}
			{#if date}<span class="mono">{date}</span>{/if}
			{#if duration}
				<span class="dot">·</span>
				<span class="mono">{humanDuration(duration)}</span>
			{/if}
			{#if downloaded}
				<span class="badge-soft">geladen</span>
			{/if}
			{#if finished}
				<span class="done">gehört</span>
			{/if}
		</p>
		{#if started}
			<p class="progress-line">
				<span class="track"><span class="fill" style="width: {percent}%"></span></span>
				<span class="left mono">noch {left}</span>
			</p>
		{/if}
	</div>
	<div class="actions">
		{#if onDownload && !downloaded}
			<button
				class="ghost-btn"
				aria-label="Folge herunterladen"
				title="Folge herunterladen"
				disabled={downloading}
				onclick={onDownload}
			>
				<Icon name="download" />
			</button>
		{/if}
		<button
			class="play"
			aria-label="{title} {started ? 'fortsetzen' : 'abspielen'}"
			title={started ? 'Fortsetzen' : 'Abspielen'}
			onclick={onPlay}
		>
			<Icon name="play-filled" />
		</button>
	</div>
</li>

<style>
	/* One card per episode: the title gets room to breathe, everything else is a quiet
	   second line, and playing is one obvious button. */
	.episode {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 12px 14px;
		border-radius: var(--radius-lg);
		border: 1px solid var(--line);
		background: var(--panel);
	}
	.episode:hover {
		border-color: color-mix(in oklab, var(--a) 40%, var(--line));
		background: var(--panel-hi, var(--panel));
	}
	.episode.finished {
		opacity: 0.62;
	}
	.cover {
		width: 56px;
		height: 56px;
		flex: none;
		border-radius: 8px;
		background: var(--tile) center/cover;
	}
	.index {
		width: 34px;
		flex: none;
		text-align: center;
		font-size: 12px;
		color: var(--faint);
	}
	.body {
		flex: 1;
		min-width: 0;
	}
	.title {
		margin: 0;
		font: 600 14px/1.35 var(--font-sans);
		/* Two lines, then ellipsis: episode titles are long and cutting at one loses the point. */
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.meta {
		margin: 4px 0 0;
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 6px;
		font-size: 11.5px;
		color: var(--faint);
	}
	.show {
		color: var(--dim);
		max-width: 220px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.show:hover {
		color: var(--a);
	}
	.dot {
		opacity: 0.5;
	}
	.badge-soft {
		padding: 1px 7px;
		border-radius: var(--radius-pill);
		background: color-mix(in oklab, var(--a) 16%, transparent);
		color: color-mix(in oklab, var(--a) 70%, var(--text));
		font-size: 10.5px;
	}
	.done {
		padding: 1px 7px;
		border-radius: var(--radius-pill);
		border: 1px solid var(--line);
		font-size: 10.5px;
	}
	.progress-line {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 7px 0 0;
	}
	.track {
		flex: 1;
		max-width: 260px;
		height: 3px;
		border-radius: 2px;
		background: var(--line);
		overflow: hidden;
	}
	.fill {
		display: block;
		height: 100%;
		background: var(--a);
	}
	.left {
		font-size: 10.5px;
		color: var(--faint);
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: none;
	}
	.ghost-btn {
		width: 34px;
		height: 34px;
		padding: 0;
		display: grid;
		place-items: center;
		border-radius: 50%;
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
		font-size: 16px;
	}
	.ghost-btn:hover:not(:disabled) {
		color: var(--text);
		border-color: var(--line-strong);
	}
	.ghost-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.play {
		width: 40px;
		height: 40px;
		padding: 0;
		border: none;
		border-radius: 50%;
		background: var(--a);
		color: var(--bg);
		font-size: 20px;
		display: grid;
		place-items: center;
	}
	.play:hover {
		filter: brightness(1.08);
	}

	@media (max-width: 700px) {
		.cover {
			width: 46px;
			height: 46px;
		}
	}
</style>
