<script lang="ts">
	import { onMount, getContext } from 'svelte';
	import { PLAYER_CONTEXT_KEY, type PlayerStore } from '$lib/player.svelte';
	import Icon from '$lib/components/Icon.svelte';
	import BrandMark from '$lib/components/BrandMark.svelte';
	import Visualizer from '$lib/components/Visualizer.svelte';
	import ChapterList from '$lib/components/ChapterList.svelte';
	import ListRow from '$lib/components/ListRow.svelte';
	import Scrubber from '$lib/components/Scrubber.svelte';
	import { shortcutFor } from '$lib/playerShortcuts';
	import PageTitle from '$lib/components/PageTitle.svelte';
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

	// The album's own title belongs in the second line — the headline is whatever is
	// actually playing right now.
	const headline = $derived(
		byTrack ? (currentTrack?.title ?? data.item.title) : (currentChapter?.title ?? data.item.title)
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

	function togglePlay() {
		if (player.current?.playing) player.pause();
		else player.resume();
	}

	/** Albums have no skip buttons, so the arrows nudge by a track-sized step there. */
	const TRACK_STEP = 10;
	const stepBack = $derived(byTrack ? TRACK_STEP : player.preferences.skipBack);
	const stepForward = $derived(byTrack ? TRACK_STEP : player.preferences.skipForward);

	function nudge(direction: -1 | 1) {
		const step = direction < 0 ? stepBack : stepForward;
		if (byTrack) player.seekInTrack(Math.max(0, player.trackOffset() + direction * step));
		else if (direction < 0) player.skipBack();
		else player.skipForward();
	}

	onMount(() => {
		function onKeydown(event: KeyboardEvent) {
			const action = shortcutFor({
				key: event.key,
				shiftKey: event.shiftKey,
				ctrlKey: event.ctrlKey,
				metaKey: event.metaKey,
				altKey: event.altKey,
				target: event.target as HTMLElement | null
			});
			if (!action) return;
			// The space bar would scroll the page, the arrows would too.
			event.preventDefault();
			if (action === 'toggle') togglePlay();
			else if (action === 'back') nudge(-1);
			else if (action === 'forward') nudge(1);
			else if (action === 'previous') goPrevious();
			else if (action === 'next') goNext();
			else close();
		}
		window.addEventListener('keydown', onKeydown);
		return () => window.removeEventListener('keydown', onKeydown);
	});

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

<PageTitle title={data.item.title} />

<div class="np" style="--a: var(--{accent})">
	<div class="ambient" aria-hidden="true">
		<span class="blob a"></span>
		<span class="blob b"></span>
		<span class="blob c"></span>
	</div>

	<div class="left">
		<div class="status-row">
			<button class="back" onclick={close} title="Zurück (Esc)">
				<Icon name="back" />
				<span>Zurück</span>
			</button>
			<BrandMark size={26} />
			<span class="eyebrow">Läuft gerade · {kindLabel}</span>
			<span class="session mono">Sitzung offen · {elapsedMinutes} Min</span>
		</div>

		<div class="hero">
			<div class="cover-row">
				<div class="cover">
					{#if data.item.coverPath}<img src="/items/{data.item.id}/cover" alt="" />{/if}
				</div>
				<div class="viz-panel">
					<Visualizer
						bars={40}
						playing={player.current?.playing ?? false}
						getAnalyser={player.getAnalyser}
					/>
				</div>
			</div>
			<h1>{headline}</h1>
			<p class="subtitle">
				{#if byTrack && currentTrack}Titel {playingIndex + 1} von {data.tracks.length} · {data.item
						.title}{:else if currentChapter}Kapitel {currentChapterIndex + 1} von {data.chapters
						.length} · {data.item.title}{/if}
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
				<div class="times mono">
					<span>{formatHMS(barElapsed)}</span>
					<span>{formatHMS(barTotal)}</span>
				</div>
			</div>

			<div class="transport">
				<div class="deck">
					<button
						class="step"
						aria-label="{stepBack} Sekunden zurück"
						title="{stepBack} s zurück (←)"
						onclick={() => nudge(-1)}
					>
						<Icon name="rewind" />
						<span class="step-value mono">{stepBack}</span>
					</button>
					<button
						class="icon-btn jump"
						aria-label={byTrack ? 'Vorheriger Titel' : 'Vorheriges Kapitel'}
						title={byTrack
							? 'Vorheriger Titel (Shift+Pfeil links)'
							: 'Vorheriges Kapitel (Shift+Pfeil links)'}
						disabled={byTrack
							? atFirstTrack && player.trackOffset() <= 3
							: currentChapterIndex <= 0}
						onclick={goPrevious}
					>
						<Icon name="previous" />
					</button>
					<button
						class="play"
						aria-label={player.current?.playing ? 'Pause' : 'Abspielen'}
						title={player.current?.playing ? 'Pause (Leertaste)' : 'Abspielen (Leertaste)'}
						onclick={togglePlay}
					>
						<Icon name={player.current?.playing ? 'pause-filled' : 'play-filled'} />
					</button>
					<button
						class="icon-btn jump"
						aria-label={byTrack ? 'Nächster Titel' : 'Nächstes Kapitel'}
						title={byTrack
							? 'Nächster Titel (Shift+Pfeil rechts)'
							: 'Nächstes Kapitel (Shift+Pfeil rechts)'}
						disabled={byTrack
							? atLastTrack
							: currentChapterIndex < 0 || currentChapterIndex >= data.chapters.length - 1}
						onclick={goNext}
					>
						<Icon name="next" />
					</button>
					<button
						class="step"
						aria-label="{stepForward} Sekunden vor"
						title="{stepForward} s vor (→)"
						onclick={() => nudge(1)}
					>
						<Icon name="forward" />
						<span class="step-value mono">{stepForward}</span>
					</button>
				</div>

				<div class="options">
					<select
						class="pill speed"
						aria-label="Geschwindigkeit"
						value={player.preferences.playbackSpeed}
						onchange={(e) => setSpeed(Number(e.currentTarget.value))}
					>
						{#each SPEEDS as value (value)}
							<option {value}>{value.toLocaleString('de-DE', { minimumFractionDigits: 2 })}×</option
							>
						{/each}
					</select>
					<select
						class="pill sleep"
						aria-label="Sleep-Timer"
						onchange={(e) => setSleepTimer(e.currentTarget.value)}
					>
						<option value="0">Sleep-Timer</option>
						<option value="15">15 Min</option>
						<option value="30">30 Min</option>
						<option value="45">45 Min</option>
						<option value="60">60 Min</option>
					</select>
				</div>
			</div>
			<p class="hints mono">
				Leertaste Play · ← {stepBack} s · → {stepForward} s · Shift+←/→ {listLabel} · Esc zurück
			</p>
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
								{#if i === playingIndex}<span class="row-viz">
										<Visualizer
											bars={4}
											playing={player.current?.playing ?? false}
											getAnalyser={player.getAnalyser}
										/>
									</span>{:else}{i + 1}{/if}
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
		/* The transport must stay on screen: the page never grows past the viewport, the
		   artwork gives way instead. */
		height: 100dvh;
		min-height: 520px;
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
		min-height: 0;
		overflow: hidden;
	}
	.status-row {
		display: flex;
		align-items: center;
		gap: 14px;
	}
	.back {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		padding: 0 14px 0 11px;
		font-size: 11.5px;
		color: var(--dim);
	}
	.back:hover {
		color: var(--text);
		border-color: var(--a);
	}
	.back :global(.icon) {
		font-size: 15px;
		flex: none;
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
		/* Takes the room between the status row and the transport, so cover and visualizer
		   grow into it instead of leaving a hole above them. */
		flex: 1 1 0;
		min-height: 0;
		justify-content: flex-end;
	}
	.cover-row {
		display: flex;
		align-items: stretch;
		gap: 24px;
		align-self: stretch;
		/* basis 0, so the square artwork sizes itself from the leftover height rather than
		   pushing the transport off screen. */
		flex: 1 1 0;
		min-height: 0;
		max-height: 42dvh;
		margin-bottom: 12px;
	}
	.viz-panel {
		flex: 1;
		min-width: 0;
		--viz-gap: 5px;
		--viz-segment: 12px;
	}
	.row-viz {
		display: block;
		width: 22px;
		height: 18px;
		--viz-gap: 2px;
		--viz-segment: 5px;
	}
	.cover {
		height: 100%;
		aspect-ratio: 1;
		max-width: 40%;
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
		margin-top: 14px;
		--scrubber-height: 6px;
		--scrubber-thumb: 12px;
	}
	.transport {
		align-self: stretch;
		width: 100%;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 18px;
		margin-top: 16px;
	}
	/* Transport first, the two dropdowns as a quieter second group beside it. */
	.deck {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.options {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.transport .pill {
		font-size: 11px;
	}
	.step {
		position: relative;
		width: 40px;
		height: 40px;
		padding: 0;
		border-radius: 50%;
		border: none;
		background: transparent;
		color: var(--dim);
		display: grid;
		place-items: center;
	}
	.step :global(.icon) {
		font-size: 30px;
	}
	.step-value {
		position: absolute;
		font-size: 9.5px;
		font-weight: 600;
		letter-spacing: -0.02em;
	}
	.step:hover,
	.jump:hover:not(:disabled) {
		color: var(--text);
	}
	.jump {
		width: 40px;
		height: 40px;
		font-size: 21px;
	}
	.play {
		width: 62px;
		height: 62px;
		border-radius: 50%;
		background: var(--a);
		border: none;
		color: var(--bg);
		font-size: 30px;
		display: grid;
		place-items: center;
		box-shadow: 0 6px 20px -8px var(--a);
	}
	.play:hover {
		filter: brightness(1.08);
	}
	.hints {
		margin: 10px 0 0;
		font-size: 10.5px;
		color: var(--faint);
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
		min-height: 0;
		overflow-y: auto;
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
	.icon-btn:disabled {
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
