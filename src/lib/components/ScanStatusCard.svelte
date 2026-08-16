<script lang="ts">
	import { onMount, onDestroy } from 'svelte';

	let status = $state<Record<string, unknown> | null>(null);
	let showLog = $state(false);
	let interval: ReturnType<typeof setInterval> | null = null;

	async function poll() {
		const res = await fetch('/scan/status');
		if (res.ok) status = await res.json();
	}
	function schedule() {
		if (interval) clearInterval(interval);
		interval = setInterval(poll, 4000);
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

	const running = $derived(Boolean(status?.running));
	const report = $derived(
		status?.last_report as {
			new?: number;
			updated?: number;
			unchanged?: number;
			skipped?: number;
		} | null
	);
	const progress = $derived(
		status?.progress as { total?: number | null; processed?: number } | null
	);
	const percent = $derived(
		progress?.total ? Math.round(((progress.processed ?? 0) / progress.total) * 100) : 0
	);
</script>

<div class="card">
	{#if running}
		<span class="eyebrow">Scan läuft</span>
		{#if progress?.total}
			<p class="stat mono">{progress.processed} Dateien · {percent} %</p>
		{/if}
	{:else}
		<span class="eyebrow">Kein Scan aktiv</span>
	{/if}
	{#if report}
		<div class="report mono">
			<span>neu {report.new ?? 0}</span>
			<span>geändert {report.updated ?? 0}</span>
			<span>unverändert {report.unchanged ?? 0}</span>
			<span>übersprungen {report.skipped ?? 0}</span>
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
