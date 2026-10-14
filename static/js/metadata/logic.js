// What the metadata editors decide, without the DOM: which request a form turns into, when a
// button may be pressed, what a summary says. Ported from legacy MetadataEditor.svelte,
// BulkEditor.svelte and the selection on the books/albums pages; tests/js/metadata.test.mjs
// covers what their component tests covered. static/js/metadata/editor.js wires it to the page.

/** A form value as it travels: trimmed text, a number for number fields, `null` when blank. */
export function toWire(type, raw) {
	const trimmed = String(raw ?? '').trim();
	if (trimmed === '') return null;
	return type === 'text' ? trimmed : Number(trimmed.replace(',', '.'));
}

/**
 * The PATCH body of the single-item editor: only fields that differ from what the page showed,
 * minus those handed back to the scanner, which go to `reset` instead. `null` when nothing is
 * to be sent.
 *
 * fields: [{ wire, type, initial, value }], unlock: wire names.
 */
export function itemPatch(fields, unlock) {
	const set = {};
	for (const field of fields) {
		if (String(field.value ?? '') === field.initial || unlock.includes(field.wire)) continue;
		set[field.wire] = toWire(field.type, field.value);
	}
	const reset = [...unlock];
	if (Object.keys(set).length === 0 && reset.length === 0) return null;
	return { set, reset };
}

/** One PATCH per track whose title was edited. tracks: [{ id, initial, value }]. */
export function trackPatches(tracks) {
	return tracks
		.filter((track) => track.value !== track.initial)
		.map((track) => ({ id: track.id, body: { set: { title: toWire('text', track.value) } } }));
}

/** Whether "Speichern" may be pressed. */
export function editorDirty(fields, unlock, tracks) {
	return (
		fields.some((field) => String(field.value ?? '') !== field.initial) ||
		unlock.length > 0 ||
		tracks.some((track) => track.value !== track.initial)
	);
}

/**
 * The bulk editor's state:
 * { target: { ids } | { filter }, fields: [{ wire, type, active, value }],
 *   replace: { field, from, to, regex }, seriesMode: 'off'|'detect'|'assign', seriesName,
 *   seriesStart }
 */
export function bulkEndpoint(state) {
	return state.seriesMode === 'off' ? '/items/bulk' : '/items/series';
}

export function bulkHasWork(state) {
	if (state.seriesMode === 'detect') return true;
	if (state.seriesMode === 'assign') return state.seriesName.trim() !== '';
	return state.fields.some((field) => field.active) || state.replace.from.trim() !== '';
}

export function bulkBody(state, dryRun) {
	const target = state.target.ids ? { ids: state.target.ids } : { filter: state.target.filter };
	if (state.seriesMode !== 'off') {
		return {
			...target,
			mode: state.seriesMode,
			...(state.seriesMode === 'assign'
				? {
						series: state.seriesName.trim(),
						// A cleared number field: the server starts at 1.
						start: String(state.seriesStart).trim() === '' ? null : Number(state.seriesStart)
					}
				: {}),
			dry_run: dryRun
		};
	}
	const set = {};
	for (const field of state.fields) {
		if (field.active) set[field.wire] = toWire(field.type, field.value);
	}
	return {
		...target,
		set,
		...(state.replace.from.trim()
			? {
					replace: {
						field: state.replace.field,
						from: state.replace.from,
						to: state.replace.to,
						regex: state.replace.regex
					}
				}
			: {}),
		dry_run: dryRun
	};
}

/** "Anwenden" only after a preview that would change something. */
export function canApply(preview, busy) {
	return Boolean(preview) && preview.changed + preview.locked_only > 0 && !busy;
}

const plural = (n, one, many) => (n === 1 ? one : many);

export function bulkHeading(count) {
	return `${count} ${plural(count, 'Eintrag', 'Einträge')} bearbeiten`;
}

export function previewText(preview) {
	if (preview.changed === 0) {
		const already =
			preview.locked_only > 0
				? ` — ${preview.locked_only} ${plural(preview.locked_only, 'Eintrag hat', 'Einträge haben')} den Wert schon`
				: '';
		return `Nichts zu ändern${already}.`;
	}
	const locked = preview.locked_only > 0 ? `, ${preview.locked_only} nur gesperrt` : '';
	return `${preview.changed} von ${preview.matched} werden geändert${locked}.`;
}

export function appliedText(result) {
	const locked = result.locked_only > 0 ? `, ${result.locked_only} nur gesperrt` : '';
	return `${result.changed} ${plural(result.changed, 'Eintrag', 'Einträge')} geändert${locked}`;
}

export function undoneText(undone) {
	const skipped =
		undone.skipped > 0 ? `, ${undone.skipped} übersprungen (inzwischen anders geändert)` : '';
	return `Zurückgenommen: ${undone.restored} ${plural(undone.restored, 'Feld', 'Felder')}${skipped}`;
}

export function seriesLead(mode) {
	if (mode === 'detect') {
		return 'Liest Serie und Band aus Ordner- und Dateinamen, wie der Scanner es täte — nur eben jetzt.';
	}
	if (mode === 'assign') return 'Nummeriert die Auswahl in der angezeigten Reihenfolge durch.';
	return '';
}

/**
 * Picking a tile: a click toggles it, a shift-click adds the range from the last pick. `order`
 * is the ids as the page shows them.
 */
export function pick(selection, order, id, shiftKey) {
	const { selected, lastPicked } = selection;
	if (shiftKey && lastPicked !== null) {
		const from = order.indexOf(lastPicked);
		const to = order.indexOf(id);
		if (from >= 0 && to >= 0) {
			const [start, end] = from < to ? [from, to] : [to, from];
			return {
				selected: [...new Set([...selected, ...order.slice(start, end + 1)])],
				lastPicked: id,
				allMatching: false
			};
		}
	}
	return {
		selected: selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id],
		lastPicked: id,
		allMatching: false
	};
}

/** The selection bar's count. */
export function selectionText(count, total, allMatching) {
	return `${allMatching && total ? `Alle ${total}` : count} ausgewählt`;
}

/** Whether to offer "Alle N auswählen". */
export function offerSelectAll(count, total, allMatching, canSelectAll) {
	return Boolean(canSelectAll && total && total > count && !allMatching);
}
