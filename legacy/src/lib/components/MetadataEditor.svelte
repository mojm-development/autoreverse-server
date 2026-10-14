<script lang="ts">
	import { untrack } from 'svelte';
	import Icon from './Icon.svelte';
	import { fieldsFor, TRACK_FIELD_LIST, type MetadataField } from '$lib/metadataFields';

	interface EditableItem {
		id: number;
		kind: string;
		lockedFields?: string[];
		[key: string]: unknown;
	}
	interface EditableTrack {
		id: number;
		position: number;
		title: string | null;
		lockedFields?: string[];
	}

	let {
		item,
		tracks = [],
		trackLabel = 'Titel',
		onClose,
		onSaved
	}: {
		item: EditableItem;
		tracks?: EditableTrack[];
		trackLabel?: string;
		onClose: () => void;
		onSaved: () => void;
	} = $props();

	const fields = $derived(fieldsFor(item.kind));
	const trackTitle = TRACK_FIELD_LIST.find((f) => f.wire === 'title')!;

	function initial(field: MetadataField): string {
		const value = item[field.column];
		return value === null || value === undefined ? '' : String(value);
	}

	// The form's own copy, taken once when the editor opens: comparing against the item
	// tells us what actually changed, so a save sends the edited fields and nothing else.
	// A number input binds a number, not a string — hence the loose type and the
	// String() comparisons below.
	let values = $state<Record<string, string | number>>(
		untrack(() => Object.fromEntries(fieldsFor(item.kind).map((f) => [f.wire, initial(f)])))
	);
	let trackValues = $state<Record<number, string>>(
		untrack(() => Object.fromEntries(tracks.map((track) => [track.id, track.title ?? ''])))
	);
	// Wire names handed back to the scanner when this form is saved.
	let unlock = $state<string[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	const locked = $derived(new Set(item.lockedFields ?? []));

	function isDirty(field: MetadataField): boolean {
		return String(values[field.wire] ?? '') !== initial(field);
	}
	const dirty = $derived(
		fields.some(isDirty) ||
			unlock.length > 0 ||
			tracks.some((track) => trackValues[track.id] !== (track.title ?? ''))
	);

	function toggleUnlock(field: MetadataField) {
		unlock = unlock.includes(field.wire)
			? unlock.filter((wire) => wire !== field.wire)
			: [...unlock, field.wire];
	}

	function toWire(field: MetadataField, raw: string | number | null): string | number | null {
		const trimmed = String(raw ?? '').trim();
		if (trimmed === '') return null;
		return field.type === 'text' ? trimmed : Number(trimmed.replace(',', '.'));
	}

	async function save() {
		saving = true;
		error = null;
		try {
			const set: Record<string, string | number | null> = {};
			for (const field of fields) {
				if (!isDirty(field) || unlock.includes(field.wire)) continue;
				set[field.wire] = toWire(field, values[field.wire]);
			}
			const reset = unlock;
			if (Object.keys(set).length > 0 || reset.length > 0) {
				const response = await fetch(`/items/${item.id}`, {
					method: 'PATCH',
					headers: { 'content-type': 'application/json' },
					body: JSON.stringify({ set, reset })
				});
				if (!response.ok)
					throw new Error((await response.json()).detail ?? 'Speichern fehlgeschlagen');
			}

			for (const track of tracks) {
				const next = trackValues[track.id];
				if (next === (track.title ?? '')) continue;
				const response = await fetch(`/tracks/${track.id}`, {
					method: 'PATCH',
					headers: { 'content-type': 'application/json' },
					body: JSON.stringify({ set: { title: toWire(trackTitle, next) } })
				});
				if (!response.ok)
					throw new Error((await response.json()).detail ?? 'Speichern fehlgeschlagen');
			}
			onSaved();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Speichern fehlgeschlagen';
		} finally {
			saving = false;
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
<div class="sheet" role="dialog" aria-modal="true" aria-label="Metadaten bearbeiten">
	<header>
		<h2>Metadaten bearbeiten</h2>
		<button class="icon-btn" aria-label="Schließen" onclick={onClose}><Icon name="expand" /></button
		>
	</header>

	<div class="body">
		<p class="lead">
			Was du hier änderst, bleibt beim nächsten Scan stehen. Ein Feld mit Schloss verwaltest du
			selbst — der Scanner fasst es nicht mehr an.
		</p>

		<div class="fields">
			{#each fields as field (field.wire)}
				<label class="field" class:reset={unlock.includes(field.wire)}>
					<span class="label">
						{field.label}
						{#if locked.has(field.column)}
							<button
								type="button"
								class="lock"
								class:active={!unlock.includes(field.wire)}
								title={unlock.includes(field.wire)
									? 'Wird beim Speichern wieder vom Scanner gesetzt'
									: 'Von Hand gesetzt — klicken, um es dem Scanner zurückzugeben'}
								onclick={() => toggleUnlock(field)}
							>
								{unlock.includes(field.wire) ? 'zurückgeben' : 'von Hand'}
							</button>
						{/if}
					</span>
					<input
						type={field.type === 'text' ? 'text' : 'number'}
						step={field.type === 'float' ? '0.5' : '1'}
						bind:value={values[field.wire]}
						disabled={unlock.includes(field.wire)}
					/>
					{#if field.hint}<span class="hint">{field.hint}</span>{/if}
				</label>
			{/each}
		</div>

		{#if tracks.length > 0}
			<h3>{trackLabel}</h3>
			<div class="tracks">
				{#each tracks as track (track.id)}
					<label class="track">
						<span class="index mono">{track.position}</span>
						<input type="text" bind:value={trackValues[track.id]} />
					</label>
				{/each}
			</div>
		{/if}
	</div>

	<footer>
		{#if error}<span class="error">{error}</span>{/if}
		<button class="outline" onclick={onClose}>Abbrechen</button>
		<button class="primary" disabled={!dirty || saving} onclick={save}>
			{saving ? 'Wird gespeichert …' : 'Speichern'}
		</button>
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
		width: min(680px, calc(100vw - 32px));
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
	.body {
		padding: 16px 18px;
		overflow-y: auto;
	}
	.lead {
		margin: 0 0 16px;
		font-size: 12px;
		color: var(--faint);
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
	}
	.label {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 11.5px;
		color: var(--dim);
	}
	/* A field the scanner no longer owns says so, and can be handed back. */
	.lock {
		height: 20px;
		padding: 0 8px;
		border-radius: var(--radius-pill);
		border: 1px solid var(--line);
		background: transparent;
		color: var(--faint);
		font: 500 10px var(--font-sans);
	}
	.lock.active {
		border-color: color-mix(in oklab, var(--a) 45%, var(--line));
		color: color-mix(in oklab, var(--a) 70%, var(--text));
	}
	.field input {
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		border: 1px solid var(--line);
		background: var(--panel);
		color: var(--text);
		font: 400 13px var(--font-sans);
	}
	.field input:disabled {
		opacity: 0.45;
	}
	.hint {
		font-size: 10.5px;
		color: var(--faint);
	}
	h3 {
		margin: 22px 0 10px;
		font: 600 12px var(--font-sans);
		color: var(--dim);
	}
	.tracks {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.track {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.track .index {
		width: 26px;
		flex: none;
		text-align: right;
		font-size: 11px;
		color: var(--faint);
	}
	.track input {
		flex: 1;
		height: 30px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		border: 1px solid var(--line);
		background: var(--panel);
		color: var(--text);
		font: 400 12.5px var(--font-sans);
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
	.primary:disabled {
		opacity: 0.45;
	}
	.outline {
		background: transparent;
		border: 1px solid var(--line-strong);
		color: var(--text);
	}
</style>
