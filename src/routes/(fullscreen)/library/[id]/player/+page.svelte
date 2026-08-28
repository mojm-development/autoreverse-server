<script lang="ts">
	import { onMount, getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import BrandMark from '$lib/components/BrandMark.svelte';
	import Visualizer from '$lib/components/Visualizer.svelte';
	import ChapterList from '$lib/components/ChapterList.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import Scrubber from '$lib/components/Scrubber.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);

	onMount(() => {
		if (player.current?.itemId !== data.item.id) {
			void player.play(data.item.id);
		}
	});

	const SPEEDS = [0.75, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];

	async function setSpeed(value: number) {
		player.setSpeed(value);
		await fetch('/me/playback', {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				playback_speed: value,
				skip_back: player.preferences.skipBack,
				skip_forward: player.preferences.skipForward
			})
		});
	}

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
	const byTrack = $derived(data.item.kind === 'album');
	const currentTrack = $derived(
		isPlayingThis ? (player.current!.tracks[player.current!.trackIndex] ?? null) : null
	);
	const barElapsed = $derived(byTrack && isPlayingThis ? player.trackOffset() : currentPosition);
	const barTotal = $derived(byTrack ? (currentTrack?.duration ?? 0) : totalDuration);
	const ticks = $derived(byTrack ? [] : data.chapters.map((c) => c.start));
	const atFirstTrack = $derived(isPlayingThis && player.current!.trackIndex === 0);
	const atLastTrack = $derived(
		isPlayingThis && player.current!.trackIndex >= player.current!.tracks.length - 1
	);
	const currentChapterIndex = $derived(
		data.chapters.findIndex((c) => currentPosition >= c.start && currentPosition < c.end)
	);
	const currentChapter = $derived(
		currentChapterIndex >= 0 ? data.chapters[currentChapterIndex] : null
	);

	let tab = $state<'chapters' | 'bookmarks'>('chapters');
	const listLabel = $derived(byTrack ? 'Titel' : 'Kapitel');
	const playingIndex = $derived(isPlayingThis ? player.current!.trackIndex : -1);

	function formatMinutes(seconds: number): string {
		return `${Math.max(1, Math.round(seconds / 60))} min`;
	}
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

	function goPrevious() {
		if (byTrack) {
			player.previousTrack();
			return;
		}
		const chapter = data.chapters[currentChapterIndex];
		if (chapter && currentPosition - chapter.start > 3) {
			playFrom(chapter.start);
			return;
		}
		const previous = data.chapters[currentChapterIndex - 1];
		if (previous) playFrom(previous.start);
	}

	function goNext() {
		if (byTrack) {
			player.nextTrack();
			return;
		}
		const next = data.chapters[currentChapterIndex + 1];
		if (next) playFrom(next.start);
	}

	function close() {
		history.back();
	}

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
			<BrandMark size={26} />
			<span class="eyebrow">Läuft gerade · {kindLabel}</span>
			<span class="session mono">Sitzung offen · {elapsedMinutes} Min</span>
		</div>

		<div class="hero">
			<div class="cover-row">
				<div class="cover">
					{#if data.item.coverPath}<img src="/items/{data.item.id}/cover" alt="" />{/if}
				</div>
				<Visualizer playing={player.current?.playing ?? false} getAnalyser={player.getAnalyser} />
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

			<div class="scrubber-wrap">
				<Scrubber
					value={barElapsed}
					max={barTotal}
					{ticks}
					label={byTrack ? 'Position im Titel' : 'Position im Hörbuch'}
					format={formatHMS}
					onSeek={(seconds) => (byTrack ? player.seekInTrack(seconds) : player.seek(seconds))}
				/>
			</div>
			<div class="times mono">
				<span>{formatHMS(barElapsed)}</span>
				<span>{formatHMS(barTotal)}</span>
			</div>

			<div class="transport">
				<select
					class="pill speed"
					aria-label="Geschwindigkeit"
					value={player.preferences.playbackSpeed}
					onchange={(e) => setSpeed(Number(e.currentTarget.value))}
				>
					{#each SPEEDS as value (value)}
						<option {value}>{value.toLocaleString('de-DE', { minimumFractionDigits: 2 })}×</option>
					{/each}
				</select>
				{#if !byTrack}
					<button
						class="skip"
						aria-label="{player.preferences.skipBack} Sekunden zurück"
						onclick={() => player.skipBack()}>{player.preferences.skipBack}</button
					>
				{/if}
				<button
					class="icon-btn"
					aria-label={byTrack ? 'Vorheriger Titel' : 'Vorheriges Kapitel'}
					disabled={byTrack ? atFirstTrack && player.trackOffset() <= 3 : currentChapterIndex <= 0}
					onclick={goPrevious}
				>
					<Icon name="previous" />
				</button>
				<button
					class="play"
					aria-label={player.current?.playing ? 'Pause' : 'Abspielen'}
					onclick={() => (player.current?.playing ? player.pause() : player.resume())}
				>
					<Icon name={player.current?.playing ? 'pause' : 'play'} />
				</button>
				<button
					class="icon-btn"
					aria-label={byTrack ? 'Nächster Titel' : 'Nächstes Kapitel'}
					disabled={byTrack
						? atLastTrack
						: currentChapterIndex < 0 || currentChapterIndex >= data.chapters.length - 1}
					onclick={goNext}
				>
					<Icon name="next" />
				</button>
				{#if !byTrack}
					<button
						class="skip"
						aria-label="{player.preferences.skipForward} Sekunden vor"
						onclick={() => player.skipForward()}>{player.preferences.skipForward}</button
					>
				{/if}
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
				>{listLabel}</button
			>
			<button class="tab" class:active={tab === 'bookmarks'} onclick={() => (tab = 'bookmarks')}>
				Lesezeichen · {data.bookmarks.length}
			</button>
		</div>
		{#if tab === 'chapters'}
			{#if byTrack}
				<div class="table" role="table" aria-label="Titel">
					{#each data.tracks as track, i (track.id)}
						<ListRow
							ariaCurrent={i === playingIndex}
							label="{track.title ?? `Titel ${i + 1}`} abspielen"
							onclick={() => player.playTrackAt(data.item.id, i)}
						>
							<span class="index mono">
								{#if i === playingIndex}<Visualizer
										playing={player.current?.playing ?? false}
										getAnalyser={player.getAnalyser}
									/>{:else}{i + 1}{/if}
							</span>
							<span class="track-title">{track.title ?? `Titel ${i + 1}`}</span>
							<span class="mono track-length">{formatMinutes(track.duration)}</span>
						</ListRow>
					{/each}
				</div>
			{:else}
				<ChapterList
					chapters={data.chapters}
					{currentPosition}
					{isPlayingThis}
					onSelect={playFrom}
				/>
			{/if}
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
	.cover-row {
		display: flex;
		align-items: flex-end;
		gap: 18px;
	}
	.cover {
		width: 300px;
		height: 300px;
		flex: none;
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
	.scrubber-wrap {
		align-self: stretch;
		width: 100%;
		max-width: 460px;
		margin-top: 14px;
		--scrubber-height: 6px;
		--scrubber-thumb: 12px;
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

	.table {
		display: flex;
		flex-direction: column;
	}
	.index {
		width: 26px;
		flex: none;
		color: var(--faint);
		display: flex;
		align-items: center;
	}
	.track-title {
		flex: 1;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		font-size: 13px;
	}
	.track-length {
		flex: none;
		color: var(--faint);
		font-size: 11px;
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
	.times {
		display: flex;
		justify-content: space-between;
		margin-top: 6px;
		font-size: 11px;
		color: var(--faint);
	}
	.icon-btn:disabled,
	.skip:disabled {
		opacity: 0.35;
		cursor: not-allowed;
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
