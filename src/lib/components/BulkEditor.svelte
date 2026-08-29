<script lang="ts">
	import Icon from './Icon.svelte';
	import { fieldsFor, type MetadataField } from '$lib/metadataFields';

	interface FieldChange {
		field: string;
		old: string | null;
		new: string | null;
	}
	interface ItemChange {
		id: number;
		title: string;
		fields: FieldChange[];
	}
	interface BulkResult {
		matched: number;
		changed: number;
		locked_only: number;
		changes: ItemChange[];
		truncated: boolean;
		batch_id: string | null;
	}

	let {
		kind,
		ids,
		filter,
		count,
		onClose,
		onApplied
	}: {
		kind: 'book' | 'album';
		/** Either the chosen items… */
		ids?: number[];
		/** …or everything a filter matches, which is how "all N" avoids sending 1599 ids. */
		filter?: Record<string, string | boolean | undefined>;
		count: number;
		onClose: () => void;
		onApplied: () => void;
	} = $props();

	// The sort key follows the title on its own; offering it here would only invite
	// a library sorted by hand.
	const fields = $derived(fieldsFor(kind).filter((field) => field.wire !== 'sort_title'));

	let values = $state<Record<string, string | number>>({});
	let active = $state<string[]>([]);
	let replaceField = $state('title');
	let replaceFrom = $state('');
	let replaceTo = $state('');
	let useRegex = $state(false);

	let preview = $state<BulkResult | null>(null);
	let applied = $state<BulkResult | null>(null);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let undone = $state<{ restored: number; skipped: number } | null>(null);

	const hasWork = $derived(active.length > 0 || replaceFrom.trim() !== '');

	function toggleField(field: MetadataField) {
		active = active.includes(field.wire)
			? active.filter((wire) => wire !== field.wire)
			: [...active, field.wire];
		preview = null;
	}

	function body(dryRun: boolean) {
		const set: Record<string, string | number | null> = {};
		for (const field of fields) {
			if (!active.includes(field.wire)) continue;
			const raw = String(values[field.wire] ?? '').trim();
			set[field.wire] =
				raw === '' ? null : field.type === 'text' ? raw : Number(raw.replace(',', '.'));
		}
		return {
			...(ids ? { ids } : { filter }),
			set,
			...(replaceFrom.trim()
				? { replace: { field: replaceField, from: replaceFrom, to: replaceTo, regex: useRegex } }
				: {}),
			dry_run: dryRun
		};
	}

	async function post(url: string, payload: unknown) {
		const response = await fetch(url, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(payload)
		});
		const data = await response.json();
		if (!response.ok) throw new Error(data.detail ?? 'Fehlgeschlagen');
		return data;
	}

	async function run(dryRun: boolean) {
		busy = true;
		error = null;
		try {
			const result = (await post('/items/bulk', body(dryRun))) as BulkResult;
			if (dryRun) preview = result;
			else {
				applied = result;
				onApplied();
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Fehlgeschlagen';
		} finally {
			busy = false;
		}
	}

	async function undo() {
		if (!applied?.batch_id) return;
		busy = true;
		error = null;
		try {
			undone = (await post('/items/bulk/undo', { batch_id: applied.batch_id })) as {
				restored: number;
				skipped: number;
			};
			applied = null;
			onApplied();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Fehlgeschlagen';
		} finally {
			busy = false;
		}
	}
</script>

<div
	class="backdrop"
	role="button"
	tabindex="-1"
	aria-label="Schließen"
	onclick={onClose}
	onkeydown={(event) => event.key === 'Escape' && onClose()}
></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label="Mehrere bearbeiten">
	<header>
		<h2>{count} {count === 1 ? 'Eintrag' : 'Einträge'} bearbeiten</h2>
		<button class="icon-btn" aria-label="Schließen" onclick={onClose}><Icon name="expand" /></button
		>
	</header>

	<div class="body">
		{#if applied}
			<div class="done">
				<p class="done-title">
					{applied.changed}
					{applied.changed === 1 ? 'Eintrag' : 'Einträge'} geändert{#if applied.locked_only > 0}, {applied.locked_only}
						nur gesperrt{/if}
				</p>
				<p class="done-text">
					Die Änderung ist gespeichert. Solange dieses Fenster offen ist, kannst du sie in einem Zug
					zurücknehmen.
				</p>
				<button class="outline" disabled={busy} onclick={undo}>Rückgängig</button>
			</div>
		{:else}
			{#if undone}
				<p class="undone">
					Zurückgenommen: {undone.restored}
					{undone.restored === 1 ? 'Feld' : 'Felder'}{#if undone.skipped > 0}, {undone.skipped}
						übersprungen (inzwischen anders geändert){/if}
				</p>
			{/if}

			<p class="lead">
				Nur angehakte Felder werden geschrieben. Was du hier setzt, gehört danach dir — der Scanner
				überschreibt es nicht mehr.
			</p>

			<div class="fields">
				{#each fields as field (field.wire)}
					<div class="field" class:on={active.includes(field.wire)}>
						<label class="check">
							<input
								type="checkbox"
								checked={active.includes(field.wire)}
								onchange={() => toggleField(field)}
							/>
							{field.label}
						</label>
						<input
							type={field.type === 'text' ? 'text' : 'number'}
							placeholder={active.includes(field.wire) ? 'leer = Feld löschen' : ''}
							disabled={!active.includes(field.wire)}
							bind:value={values[field.wire]}
							oninput={() => (preview = null)}
						/>
					</div>
				{/each}
			</div>

			<h3>Suchen und ersetzen</h3>
			<div class="replace">
				<select bind:value={replaceField} onchange={() => (preview = null)}>
					{#each fields.filter((f) => f.type === 'text') as field (field.wire)}
						<option value={field.wire}>{field.label}</option>
					{/each}
				</select>
				<input placeholder="Suchen" bind:value={replaceFrom} oninput={() => (preview = null)} />
				<input
					placeholder="Ersetzen durch"
					bind:value={replaceTo}
					oninput={() => (preview = null)}
				/>
				<label class="check">
					<input type="checkbox" bind:checked={useRegex} onchange={() => (preview = null)} />
					Regex
				</label>
			</div>

			{#if preview}
				<h3>Vorschau</h3>
				{#if preview.changed === 0}
					<p class="empty">
						Nichts zu ändern{#if preview.locked_only > 0}
							— {preview.locked_only}
							{preview.locked_only === 1 ? 'Eintrag hat' : 'Einträge haben'} den Wert schon{/if}.
					</p>
				{:else}
					<p class="preview-count">
						{preview.changed} von {preview.matched} werden geändert{#if preview.locked_only > 0}, {preview.locked_only}
							nur gesperrt{/if}.
					</p>
					<div class="diff">
						{#each preview.changes as change (change.id)}
							<div class="row">
								<span class="row-title">{change.title}</span>
								<span class="row-fields">
									{#each change.fields as field (field.field)}
										<span class="pair">
											<span class="old">{field.old ?? '—'}</span>
											<span class="arrow">→</span>
											<span class="new">{field.new ?? '—'}</span>
										</span>
									{/each}
								</span>
							</div>
						{/each}
						{#if preview.truncated}
							<p class="more">… weitere werden ebenfalls geändert.</p>
						{/if}
					</div>
				{/if}
			{/if}
		{/if}
	</div>

	<footer>
		{#if error}<span class="error">{error}</span>{/if}
		<button class="outline" onclick={onClose}>{applied ? 'Fertig' : 'Abbrechen'}</button>
		{#if !applied}
			<button class="outline" disabled={!hasWork || busy} onclick={() => run(true)}>Vorschau</button
			>
			<button
				class="primary"
				disabled={!preview || preview.changed + preview.locked_only === 0 || busy}
				onclick={() => run(false)}
			>
				{busy ? 'Läuft …' : 'Anwenden'}
			</button>
		{/if}
	</footer>
</div>

<style>
	.backdrop {
		position: fixed;
		inset: 0;
		z-index: 40;
		background: rgb(0 0 0 / 0.55);
		border: none;
		padding: 0;
	}
	.sheet {
		position: fixed;
		z-index: 41;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
		width: min(760px, calc(100vw - 32px));
		max-height: min(86dvh, 900px);
		display: flex;
		flex-direction: column;
		border-radius: var(--radius-lg);
		border: 1px solid var(--line);
		background: var(--sidebar);
		box-shadow: 0 30px 80px -30px rgb(0 0 0 / 0.8);
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 16px 18px;
		border-bottom: 1px solid var(--line);
	}
	h2 {
		margin: 0;
		font: 600 15px var(--font-sans);
	}
	h3 {
		margin: 22px 0 10px;
		font: 600 12px var(--font-sans);
		color: var(--dim);
	}
	.body {
		padding: 16px 18px;
		overflow-y: auto;
	}
	.lead,
	.empty,
	.preview-count,
	.more,
	.undone {
		font-size: 12px;
		color: var(--faint);
		margin: 0 0 14px;
	}
	.preview-count {
		color: var(--dim);
	}
	.fields {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: 12px 14px;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: 5px;
		min-width: 0;
		opacity: 0.6;
	}
	.field.on {
		opacity: 1;
	}
	.check {
		display: flex;
		align-items: center;
		gap: 7px;
		font-size: 11.5px;
		color: var(--dim);
	}
	.field input[type='text'],
	.field input[type='number'],
	.replace input:not([type='checkbox']),
	.replace select {
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		border: 1px solid var(--line);
		background: var(--panel);
		color: var(--text);
		font: 400 13px var(--font-sans);
	}
	.replace {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}
	.replace input:not([type='checkbox']) {
		flex: 1;
		min-width: 140px;
	}
	/* Checkboxes keep their own size — the field styling above would blow them up. */
	.check input[type='checkbox'] {
		width: 14px;
		height: 14px;
		flex: none;
		margin: 0;
	}
	/* The diff is the point of the preview: old on the left, new on the right. */
	.diff {
		display: flex;
		flex-direction: column;
		gap: 4px;
		max-height: 260px;
		overflow-y: auto;
		border: 1px solid var(--line);
		border-radius: var(--radius-md);
		padding: 8px;
	}
	.row {
		display: flex;
		align-items: baseline;
		gap: 12px;
		font-size: 12px;
	}
	.row-title {
		flex: 1;
		min-width: 0;
		color: var(--dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-fields {
		flex: 2;
		display: flex;
		flex-wrap: wrap;
		gap: 10px;
		min-width: 0;
	}
	.pair {
		display: inline-flex;
		align-items: baseline;
		gap: 6px;
		min-width: 0;
	}
	.old {
		color: var(--faint);
		text-decoration: line-through;
	}
	.arrow {
		color: var(--faint);
	}
	.new {
		color: var(--text);
	}
	.done {
		text-align: center;
		padding: 20px 0;
	}
	.done-title {
		margin: 0 0 6px;
		font: 600 14px var(--font-sans);
	}
	.done-text {
		margin: 0 0 14px;
		font-size: 12px;
		color: var(--faint);
	}
	footer {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 10px;
		padding: 14px 18px;
		border-top: 1px solid var(--line);
	}
	.error {
		margin-right: auto;
		color: var(--danger);
		font-size: 12px;
	}
	.primary,
	.outline {
		height: 32px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		font: 500 12.5px var(--font-sans);
		border: none;
	}
	.primary {
		background: var(--a);
		color: var(--bg);
	}
	.primary:disabled,
	.outline:disabled {
		opacity: 0.45;
	}
	.outline {
		background: transparent;
		border: 1px solid var(--line-strong);
		color: var(--text);
	}
</style>
