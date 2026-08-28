<script lang="ts">
	import { onMount, onDestroy } from 'svelte';

	let status = $state<Record<string, unknown> | null>(null);
	let showLog = $state(false);
	let interval: ReturnType<typeof setInterval> | null = null;

	const RUNNING_MS = 1000;
	const IDLE_MS = 5000;
	let period = $state(IDLE_MS);

	async function poll() {
		const res = await fetch('/scan/status');
		if (!res.ok) return;
		status = await res.json();
		const wanted = status?.running ? RUNNING_MS : IDLE_MS;
		if (wanted !== period) {
			period = wanted;
			schedule();
		}
	}
	function schedule() {
		if (interval) clearInterval(interval);
		interval = setInterval(poll, period);
	}
	onMount(() => {
		void poll();
		schedule();
	});
	onDestroy(() => {
		if (interval) clearInterval(interval);
	});

	async function start() {
		await fetch('/scan', { method: 'POST' });
		void poll();
	}
	async function cancel() {
		await fetch('/scan/cancel', { method: 'POST' });
		void poll();
	}

	interface Counts {
		new?: number;
		updated?: number;
		unchanged?: number;
		skipped?: number;
	}
	interface Progress extends Counts {
		phase?: 'scanning' | 'storing';
		root?: string | null;
		total?: number | null;
		processed?: number;
	}

	const running = $derived(Boolean(status?.running));
	const report = $derived(status?.last_report as Counts | null);
	const progress = $derived(status?.progress as Progress | null);

	const determinate = $derived(Boolean(progress?.total));
	const percent = $derived(
		progress?.total
			? Math.min(100, Math.round(((progress.processed ?? 0) / progress.total) * 100))
			: 0
	);
	const phaseLabel = $derived(
		progress?.phase === 'storing' ? 'Wird gespeichert' : 'Ordner werden gelesen'
	);
	const rootLabel = $derived(
		progress?.root ? progress.root.replace(/\/+$/, '').split('/').pop() : null
	);
	const counts = $derived<Counts | null>(running ? (progress ?? report) : report);
	const statLine = $derived(
		[
			phaseLabel,
			rootLabel,
			determinate ? `${progress?.processed ?? 0} von ${progress?.total}` : null
		]
			.filter(Boolean)
			.join(' · ')
	);
</script>

<div class="card">
	{#if running}
		<span class="eyebrow">Scan läuft</span>
		{#if progress}
			<p class="stat mono">{statLine}</p>
			<div
				class="bar"
				class:indeterminate={!determinate}
				role="progressbar"
				aria-label="Scan-Fortschritt"
				aria-valuemin={determinate ? 0 : undefined}
				aria-valuemax={determinate ? 100 : undefined}
				aria-valuenow={determinate ? percent : undefined}
			>
				<div class="fill" style={determinate ? `width: ${percent}%` : undefined}></div>
			</div>
		{/if}
	{:else}
		<span class="eyebrow">Kein Scan aktiv</span>
	{/if}
	{#if counts}
		<div class="report mono">
			<span>neu {counts.new ?? 0}</span>
			<span>geändert {counts.updated ?? 0}</span>
			<span>unverändert {counts.unchanged ?? 0}</span>
			<span>übersprungen {counts.skipped ?? 0}</span>
		</div>
	{/if}
	<div class="actions">
		{#if running}
			<button class="outline" onclick={cancel}>Abbrechen</button>
		{:else}
			<button class="primary" onclick={start}>Scan starten</button>
		{/if}
		<button class="link" onclick={() => (showLog = !showLog)}>Protokoll</button>
	</div>
	{#if showLog}
		<div class="log mono">
			{#if status?.last_error}<p>Fehler: {status.last_error}</p>{/if}
			<p>Übersprungen: {((status?.last_skipped as string[]) ?? []).join(', ') || '—'}</p>
		</div>
	{/if}
</div>

<style>
	.card {
		padding: 16px;
		border-radius: var(--radius-lg);
		background: var(--panel);
		border: 1px solid var(--line);
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.stat {
		color: var(--dim);
		font-size: 12px;
		margin: 0;
	}
	.bar {
		height: 4px;
		border-radius: var(--radius-pill);
		background: var(--line);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		background: var(--a);
		border-radius: inherit;
		transition: width 240ms ease-out;
	}
	.bar.indeterminate .fill {
		width: 35%;
		transition: none;
		animation: sweep 1.1s ease-in-out infinite;
	}
	@keyframes sweep {
		0% {
			transform: translateX(-100%);
		}
		100% {
			transform: translateX(calc(100% / 0.35));
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.bar.indeterminate .fill {
			animation: none;
			width: 100%;
			opacity: 0.4;
		}
		.fill {
			transition: none;
		}
	}
	.report {
		display: flex;
		gap: 14px;
		color: var(--faint);
		font-size: 11px;
	}
	.actions {
		display: flex;
		gap: 8px;
		margin-top: 4px;
	}
	.primary,
	.outline {
		height: 30px;
		padding: 0 14px;
		border-radius: var(--radius-pill);
		font: 500 12px var(--font-sans);
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
	.link {
		background: none;
		border: none;
		color: var(--dim);
		font-size: 11px;
		text-decoration: underline;
	}
	.log {
		color: var(--faint);
		font-size: 11px;
		white-space: pre-wrap;
	}
</style>
