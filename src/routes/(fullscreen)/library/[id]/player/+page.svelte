<script lang="ts">
	import { onMount, getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import ChapterList from '$lib/components/ChapterList.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);

	onMount(() => {
		if (player.current?.itemId !== data.item.id) {
			void player.play(data.item.id);
		}
	});

	function playFrom(position: number) {
		void player.playFrom(data.item.id, position);
	}

	function accentFor(kind: string): 'book' | 'music' | 'podcast' {
		if (kind === 'book') return 'book';
		if (kind === 'album') return 'music';
		return 'podcast';
	}
	const accent = $derived(accentFor(data.item.kind));
	const kindLabel = $derived(
		data.item.kind === 'book' ? 'Hörbuch' : data.item.kind === 'album' ? 'Album' : 'Podcast'
	);

	const isPlayingThis = $derived(player.current?.itemId === data.item.id);
	const currentPosition = $derived(isPlayingThis ? player.current!.position : 0);
	const totalDuration = $derived(
		isPlayingThis ? player.current!.tracks.reduce((s, t) => s + t.duration, 0) : 0
	);
	const percent = $derived(totalDuration > 0 ? (currentPosition / totalDuration) * 100 : 0);
	const currentChapterIndex = $derived(
		data.chapters.findIndex((c) => currentPosition >= c.start && currentPosition < c.end)
	);
	const currentChapter = $derived(
		currentChapterIndex >= 0 ? data.chapters[currentChapterIndex] : null
	);

	let tab = $state<'chapters' | 'bookmarks'>('chapters');
	let sessionStart = Date.now();
	let elapsedMinutes = $state(0);
	onMount(() => {
		const interval = setInterval(() => {
			elapsedMinutes = Math.round((Date.now() - sessionStart) / 60_000);
		}, 30_000);
		return () => clearInterval(interval);
	});

	function formatHMS(seconds: number): string {
		const h = Math.floor(seconds / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const s = Math.floor(seconds % 60);
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${h}:${pad(m)}:${pad(s)}`;
	}

	function close() {
		history.back();
	}

	// ponytail: no backend/persisted sleep-timer concept exists anywhere in this
	// plan's scope — a real client-side setTimeout calling player.pause() is the
	// most that's achievable with zero new server surface, per the brief's own
	// explicit instruction to build exactly this much and no more.
	let sleepTimerHandle: ReturnType<typeof setTimeout> | null = null;
	function setSleepTimer(minutes: string) {
		if (sleepTimerHandle) clearTimeout(sleepTimerHandle);
		sleepTimerHandle = null;
		const m = Number(minutes);
		if (m > 0) {
			sleepTimerHandle = setTimeout(() => player.pause(), m * 60_000);
		}
	}
</script>

<div class="np" style="--a: var(--{accent})">
	<div class="ambient" aria-hidden="true">
		<span class="blob a"></span>
		<span class="blob b"></span>
		<span class="blob c"></span>
	</div>

	<div class="left">
		<div class="status-row">
			<button class="close" aria-label="Schließen" onclick={close}><Icon name="expand" /></button>
			<span class="eyebrow">Läuft gerade · {kindLabel}</span>
			<span class="session mono">Sitzung offen · {elapsedMinutes} Min</span>
		</div>

		<div class="hero">
			<div class="cover">
				{#if data.item.coverPath}<img src="/items/{data.item.id}/cover" alt="" />{/if}
			</div>
			<h1>{currentChapter?.title ?? data.item.title}</h1>
			<p class="subtitle">
				{#if currentChapter}Kapitel {currentChapterIndex + 1} von {data.chapters.length} · {data
						.item.title}{/if}
			</p>
			<p class="byline">
				{data.item.author ?? data.item.artist ?? ''}{#if data.item.narrator}
					· {data.item.narrator}{/if}
			</p>

			<div class="scrubber">
				{#each data.chapters as c (c.title + c.start)}
					<span
						class="tick"
						style="left: {totalDuration > 0 ? (c.start / totalDuration) * 100 : 0}%"
					></span>
				{/each}
				<div class="fill" style="width: {percent}%"></div>
			</div>

			<div class="transport">
				<select class="pill speed" value={player.current?.speed ?? 1} disabled>
					<option value={1}>1,00×</option>
				</select>
				<button class="skip" onclick={() => player.skipBack(30)}>30</button>
				<button class="icon-btn" aria-label="Vorheriges Kapitel"><Icon name="previous" /></button>
				<button
					class="play"
					aria-label={player.current?.playing ? 'Pause' : 'Abspielen'}
					onclick={() => (player.current?.playing ? player.pause() : player.resume())}
				>
					<Icon name={player.current?.playing ? 'pause' : 'play'} />
				</button>
				<button class="icon-btn" aria-label="Nächstes Kapitel"><Icon name="next" /></button>
				<button class="skip" onclick={() => player.skipForward(15)}>15</button>
				<select class="pill sleep" onchange={(e) => setSleepTimer(e.currentTarget.value)}>
					<option value="0">Sleep-Timer</option>
					<option value="15">15 Min</option>
					<option value="30">30 Min</option>
					<option value="45">45 Min</option>
					<option value="60">60 Min</option>
				</select>
			</div>
		</div>
	</div>

	<div class="right">
		<div class="tabs">
			<button class="tab" class:active={tab === 'chapters'} onclick={() => (tab = 'chapters')}
				>Kapitel</button
			>
			<button class="tab" class:active={tab === 'bookmarks'} onclick={() => (tab = 'bookmarks')}>
				Lesezeichen · {data.bookmarks.length}
			</button>
		</div>
		{#if tab === 'chapters'}
			<ChapterList chapters={data.chapters} {currentPosition} {isPlayingThis} onSelect={playFrom} />
		{:else}
			<div class="table" role="table" aria-label="Lesezeichen">
				{#each data.bookmarks as b (b.id)}
					<button
						type="button"
						class="bookmark-row bare"
						role="row"
						onclick={() => playFrom(b.position)}
					>
						<span>{b.title}</span>
						<span class="mono">{formatHMS(b.position)}</span>
					</button>
				{/each}
			</div>
		{/if}
		{#if data.bookmarks.length > 0}
			<div class="bookmark-card">
				<span class="eyebrow">Zuletzt gesetzt</span>
				<span class="title">{data.bookmarks[data.bookmarks.length - 1].title}</span>
			</div>
		{/if}
	</div>
</div>

<style>
	.np {
		position: relative;
		display: grid;
		grid-template-columns: 1fr 360px;
		min-height: 100vh;
		background: var(--bg);
		overflow: hidden;
	}
	.ambient {
		position: absolute;
		inset: 0;
		z-index: 0;
		overflow: hidden;
	}
	.blob {
		position: absolute;
		width: 60vw;
		height: 60vw;
		border-radius: 50%;
		filter: blur(90px);
		opacity: 0.16;
	}
	.blob.a {
		background: var(--a);
		top: -10%;
		left: -10%;
		animation: npDriftA 26s ease-in-out infinite alternate;
	}
	.blob.b {
		background: var(--music);
		bottom: -15%;
		right: -5%;
		animation: npDriftB 32s ease-in-out infinite alternate;
	}
	.blob.c {
		background: var(--podcast);
		top: 30%;
		right: 20%;
		animation: npDriftC 40s ease-in-out infinite alternate;
	}
	@keyframes npDriftA {
		from {
			transform: translate(0, 0) scale(1);
		}
		to {
			transform: translate(6%, 4%) scale(1.08);
		}
	}
	@keyframes npDriftB {
		from {
			transform: translate(0, 0) scale(1);
		}
		to {
			transform: translate(-5%, -6%) scale(1.05);
		}
	}
	@keyframes npDriftC {
		from {
			transform: translate(0, 0) scale(1);
		}
		to {
			transform: translate(4%, -3%) scale(0.95);
		}
	}

	.left {
		position: relative;
		z-index: 1;
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		padding: 28px 40px;
	}
	.status-row {
		display: flex;
		align-items: center;
		gap: 14px;
	}
	.close {
		width: 32px;
		height: 32px;
		border-radius: 50%;
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
	}
	.session {
		margin-left: auto;
		color: var(--faint);
		font-size: 11px;
	}
	.hero {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 10px;
	}
	.cover {
		width: 300px;
		height: 300px;
		border-radius: var(--radius-lg);
		background: var(--tile);
		overflow: hidden;
	}
	.cover img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	h1 {
		font: 600 46px/1.1 var(--font-sans);
		margin: 4px 0 0;
	}
	.subtitle {
		color: var(--dim);
		margin: 0;
	}
	.byline {
		color: var(--faint);
		margin: 0;
		font-size: 12px;
	}
	.scrubber {
		position: relative;
		width: 100%;
		height: 6px;
		border-radius: 99px;
		background: var(--track);
		margin-top: 14px;
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
	.transport {
		display: flex;
		align-items: center;
		gap: 14px;
		margin-top: 18px;
	}
	.transport .pill {
		font-size: 11px;
	}
	.skip {
		width: 34px;
		height: 34px;
		border-radius: 50%;
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
		font: 600 10px var(--font-mono);
	}
	.play {
		width: 64px;
		height: 64px;
		border-radius: 50%;
		background: var(--a);
		border: none;
		display: grid;
		place-items: center;
	}

	.right {
		position: relative;
		z-index: 1;
		background: var(--sidebar);
		border-left: 1px solid var(--line);
		padding: 24px 20px;
		display: flex;
		flex-direction: column;
	}
	.tabs {
		display: flex;
		gap: 18px;
		border-bottom: 1px solid var(--line);
		padding-bottom: 8px;
		margin-bottom: 10px;
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
	.bookmark-row {
		display: flex;
		justify-content: space-between;
		padding: 8px 6px;
	}
	.bookmark-card {
		margin-top: auto;
		padding: 12px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.bookmark-card .title {
		font: 500 12.5px var(--font-sans);
	}
</style>
