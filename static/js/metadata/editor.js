// The metadata editors in the page (legacy MetadataEditor.svelte, BulkEditor.svelte,
// SelectionBar.svelte and the picking on the books/albums pages). The server renders each sheet
// into a <template>; opening one clones it into <body> — outside #page, so the page can be
// reloaded underneath an open sheet (after "Anwenden") without losing it. Everything is
// delegated, so pages swapped in by htmx need no re-binding. The decisions live in logic.js.

import {
	appliedText,
	bulkBody,
	bulkEndpoint,
	bulkHasWork,
	bulkHeading,
	canApply,
	editorDirty,
	itemPatch,
	offerSelectAll,
	pick,
	previewText,
	selectionText,
	seriesLead,
	trackPatches,
	undoneText
} from './logic.js';

const JSON_HEADERS = { 'content-type': 'application/json' };

function elementOf(event) {
	const target = event.target;
	if (target instanceof Element) return target;
	return target?.parentElement ?? null;
}

async function detailOf(response, fallback) {
	try {
		const body = await response.json();
		return typeof body?.detail === 'string' ? body.detail : fallback;
	} catch {
		return fallback;
	}
}

async function send(method, url, body) {
	let response;
	try {
		response = await fetch(url, { method, headers: JSON_HEADERS, body: JSON.stringify(body) });
	} catch {
		throw new Error('Keine Verbindung zum Server.');
	}
	if (!response.ok) throw new Error(await detailOf(response, 'Fehlgeschlagen'));
	return response.json();
}

/** Reloads #page in place (the player outside it keeps playing). */
function refreshPage() {
	const htmx = window.htmx;
	if (!htmx) {
		location.reload();
		return Promise.resolve();
	}
	return htmx.ajax('GET', location.pathname + location.search, {
		target: '#page',
		select: '#page',
		swap: 'outerHTML'
	});
}

// --- sheets ---

function openSheet(selector) {
	const template = document.querySelector(selector);
	if (!(template instanceof HTMLTemplateElement)) return null;
	closeSheets();
	const root = template.content.firstElementChild?.cloneNode(true);
	if (!(root instanceof HTMLElement)) return null;
	// The page's accent (book or music) comes along into <body>.
	const accented = template.closest('[style*="--a"]');
	if (accented) root.style.setProperty('--a', getComputedStyle(accented).getPropertyValue('--a'));
	document.body.append(root);
	root.querySelector('input:not([disabled]), button.primary')?.focus();
	return root;
}

function closeSheets() {
	for (const sheet of document.querySelectorAll('[data-sheet]')) sheet.remove();
}

function showError(sheet, message) {
	const error = sheet.querySelector('[data-sheet-error]');
	if (!error) return;
	error.textContent = message ?? '';
	error.hidden = !message;
}

// --- single item (MetadataEditor) ---

function editorFields(sheet) {
	return [...sheet.querySelectorAll('.field[data-field]')].map((label) => ({
		label,
		wire: label.dataset.field,
		type: label.dataset.type,
		initial: label.dataset.initial ?? '',
		value: label.querySelector('input')?.value ?? ''
	}));
}

function editorTracks(sheet) {
	return [...sheet.querySelectorAll('input[data-track-id]')].map((input) => ({
		id: Number(input.dataset.trackId),
		initial: input.dataset.initial ?? '',
		value: input.value
	}));
}

function editorUnlock(sheet) {
	return [...sheet.querySelectorAll('.field.reset[data-field]')].map((f) => f.dataset.field);
}

function renderEditor(sheet) {
	const save = sheet.querySelector('[data-editor-save]');
	if (!save || sheet.dataset.busy) return;
	save.disabled = !editorDirty(editorFields(sheet), editorUnlock(sheet), editorTracks(sheet));
}

function toggleLock(sheet, button) {
	const field = button.closest('.field');
	if (!field) return;
	const unlocking = !field.classList.contains('reset');
	field.classList.toggle('reset', unlocking);
	button.classList.toggle('active', !unlocking);
	button.textContent = unlocking ? 'zurückgeben' : 'von Hand';
	button.title = unlocking
		? 'Wird beim Speichern wieder vom Scanner gesetzt'
		: 'Von Hand gesetzt — klicken, um es dem Scanner zurückzugeben';
	const input = field.querySelector('input');
	if (input) input.disabled = unlocking;
	renderEditor(sheet);
}

async function saveEditor(sheet) {
	const save = sheet.querySelector('[data-editor-save]');
	sheet.dataset.busy = '1';
	if (save) {
		save.disabled = true;
		save.textContent = 'Wird gespeichert …';
	}
	showError(sheet, null);
	try {
		const patch = itemPatch(editorFields(sheet), editorUnlock(sheet));
		if (patch) await send('PATCH', `/items/${sheet.dataset.itemId}`, patch);
		for (const track of trackPatches(editorTracks(sheet))) {
			await send('PATCH', `/tracks/${track.id}`, track.body);
		}
		sheet.remove();
		await refreshPage();
	} catch (error) {
		showError(sheet, error instanceof Error ? error.message : 'Speichern fehlgeschlagen');
	} finally {
		delete sheet.dataset.busy;
		if (save) save.textContent = 'Speichern';
		renderEditor(sheet);
	}
}

// --- selection (SelectionBar + the pick buttons on the tiles) ---

let selection = { page: null, selected: [], lastPicked: null, allMatching: false };

function currentSelection() {
	const page = document.getElementById('page');
	if (selection.page !== page) {
		selection = { page, selected: [], lastPicked: null, allMatching: false };
	}
	return selection;
}

function selectionBar() {
	return document.querySelector('[data-selection-bar]');
}

function renderSelection() {
	const state = currentSelection();
	for (const button of document.querySelectorAll('[data-pick]')) {
		const on = state.allMatching || state.selected.includes(Number(button.dataset.pick));
		button.classList.toggle('on', on);
		button.setAttribute('aria-pressed', String(on));
	}
	const bar = selectionBar();
	if (!bar) return;
	const count = state.selected.length;
	const total = Number(bar.dataset.total) || 0;
	bar.hidden = count === 0 && !state.allMatching;
	const label = bar.querySelector('[data-selection-count]');
	if (label) label.textContent = selectionText(count, total, state.allMatching);
	const all = bar.querySelector('[data-select-all]');
	if (all) {
		all.hidden = !offerSelectAll(count, total, state.allMatching, 'filter' in bar.dataset);
		all.textContent = `Alle ${total} auswählen`;
	}
}

function visibleOrder() {
	return [...document.querySelectorAll('[data-pick]')]
		.filter((button) => !button.closest('[hidden]'))
		.map((button) => Number(button.dataset.pick));
}

function clearSelection() {
	const state = currentSelection();
	state.selected = [];
	state.lastPicked = null;
	state.allMatching = false;
	renderSelection();
}

// --- many items (BulkEditor) ---

function bulkState(sheet) {
	return {
		target: JSON.parse(sheet.dataset.target),
		fields: [...sheet.querySelectorAll('.field[data-field]')].map((field) => ({
			wire: field.dataset.field,
			type: field.dataset.type,
			active: field.querySelector('[data-field-toggle]')?.checked ?? false,
			value: field.querySelector('[data-field-value]')?.value ?? ''
		})),
		replace: {
			field: sheet.querySelector('[data-replace-field]')?.value ?? 'title',
			from: sheet.querySelector('[data-replace-from]')?.value ?? '',
			to: sheet.querySelector('[data-replace-to]')?.value ?? '',
			regex: sheet.querySelector('[data-replace-regex]')?.checked ?? false
		},
		seriesMode: sheet.querySelector('[data-series-mode]:checked')?.value ?? 'off',
		seriesName: sheet.querySelector('[data-series-name]')?.value ?? '',
		seriesStart: sheet.querySelector('[data-series-start]')?.value ?? '1'
	};
}

/** Per sheet: the last preview and the last applied result. */
const bulkResults = new WeakMap();

function renderBulk(sheet) {
	const state = bulkState(sheet);
	const results = bulkResults.get(sheet) ?? {};
	const busy = Boolean(sheet.dataset.busy);
	const applied = results.applied ?? null;

	for (const field of sheet.querySelectorAll('.field[data-field]')) {
		const on = field.querySelector('[data-field-toggle]')?.checked ?? false;
		field.classList.toggle('on', on);
		const input = field.querySelector('[data-field-value]');
		if (input) {
			input.disabled = !on;
			input.placeholder = on ? 'leer = Feld löschen' : '';
		}
	}
	sheet.querySelector('[data-bulk-fields]')?.classList.toggle('dimmed', state.seriesMode !== 'off');
	const assign = state.seriesMode === 'assign';
	for (const input of sheet.querySelectorAll('[data-series-name], [data-series-start]')) {
		input.hidden = !assign;
	}
	const lead = sheet.querySelector('[data-series-lead]');
	if (lead) {
		lead.textContent = seriesLead(state.seriesMode);
		lead.hidden = state.seriesMode === 'off';
	}

	const done = sheet.querySelector('[data-bulk-done]');
	const form = sheet.querySelector('[data-bulk-form]');
	if (done) done.hidden = !applied;
	if (form) form.hidden = Boolean(applied);
	if (applied) {
		const title = sheet.querySelector('[data-bulk-done-title]');
		if (title) title.textContent = appliedText(applied);
	}
	const undo = sheet.querySelector('[data-bulk-undo]');
	if (undo) undo.disabled = busy;

	const cancel = sheet.querySelector('[data-bulk-cancel]');
	if (cancel) cancel.textContent = applied ? 'Fertig' : 'Abbrechen';
	const previewButton = sheet.querySelector('[data-bulk-preview-button]');
	if (previewButton) {
		previewButton.hidden = Boolean(applied);
		previewButton.disabled = !bulkHasWork(state) || busy;
	}
	const apply = sheet.querySelector('[data-bulk-apply]');
	if (apply) {
		apply.hidden = Boolean(applied);
		apply.disabled = !canApply(results.preview, busy);
		apply.textContent = busy ? 'Läuft …' : 'Anwenden';
	}
}

function renderPreview(sheet, preview) {
	const area = sheet.querySelector('[data-bulk-preview]');
	if (!area) return;
	area.replaceChildren();
	if (!preview) return;
	const heading = document.createElement('h3');
	heading.textContent = 'Vorschau';
	const summary = document.createElement('p');
	summary.className = preview.changed === 0 ? 'empty' : 'preview-count';
	summary.textContent = previewText(preview);
	area.append(heading, summary);
	if (preview.changed === 0) return;
	const diff = document.createElement('div');
	diff.className = 'diff';
	for (const change of preview.changes) {
		const row = document.createElement('div');
		row.className = 'row';
		const title = document.createElement('span');
		title.className = 'row-title';
		title.textContent = change.title;
		const fields = document.createElement('span');
		fields.className = 'row-fields';
		for (const field of change.fields) {
			const pair = document.createElement('span');
			pair.className = 'pair';
			for (const [cls, text] of [
				['old', field.old ?? '—'],
				['arrow', '→'],
				['new', field.new ?? '—']
			]) {
				const part = document.createElement('span');
				part.className = cls;
				part.textContent = text;
				pair.append(part);
			}
			fields.append(pair);
		}
		row.append(title, fields);
		diff.append(row);
	}
	if (preview.truncated) {
		const more = document.createElement('p');
		more.className = 'more';
		more.textContent = '… weitere werden ebenfalls geändert.';
		diff.append(more);
	}
	area.append(diff);
}

/** Any change to the form makes the shown preview stale. */
function invalidatePreview(sheet) {
	const results = bulkResults.get(sheet) ?? {};
	if (results.preview) {
		results.preview = null;
		bulkResults.set(sheet, results);
		renderPreview(sheet, null);
	}
	renderBulk(sheet);
}

async function runBulk(sheet, dryRun) {
	const results = bulkResults.get(sheet) ?? {};
	const state = bulkState(sheet);
	sheet.dataset.busy = '1';
	showError(sheet, null);
	renderBulk(sheet);
	try {
		const result = await send('POST', bulkEndpoint(state), bulkBody(state, dryRun));
		if (dryRun) {
			results.preview = result;
			renderPreview(sheet, result);
		} else {
			results.applied = result;
			results.preview = null;
			renderPreview(sheet, null);
			const undone = sheet.querySelector('[data-bulk-undone]');
			if (undone) undone.hidden = true;
		}
		bulkResults.set(sheet, results);
		if (!dryRun) await refreshPage();
	} catch (error) {
		showError(sheet, error instanceof Error ? error.message : 'Fehlgeschlagen');
	} finally {
		delete sheet.dataset.busy;
		renderBulk(sheet);
	}
}

async function undoBulk(sheet) {
	const results = bulkResults.get(sheet) ?? {};
	if (!results.applied?.batch_id) return;
	sheet.dataset.busy = '1';
	showError(sheet, null);
	renderBulk(sheet);
	try {
		const undone = await send('POST', '/items/bulk/undo', { batch_id: results.applied.batch_id });
		results.applied = null;
		bulkResults.set(sheet, results);
		const line = sheet.querySelector('[data-bulk-undone]');
		if (line) {
			line.textContent = undoneText(undone);
			line.hidden = false;
		}
		await refreshPage();
	} catch (error) {
		showError(sheet, error instanceof Error ? error.message : 'Fehlgeschlagen');
	} finally {
		delete sheet.dataset.busy;
		renderBulk(sheet);
	}
}

/**
 * "Rückgängig" above the list: the last bulk change after its sheet was closed. The page is
 * reloaded (titles, sort order); the result goes into the fresh page's status line.
 */
async function undoLastBulk(button) {
	const batchId = button.dataset.batchId;
	if (!batchId || button.disabled) return;
	button.disabled = true;
	let message;
	try {
		message = undoneText(await send('POST', '/items/bulk/undo', { batch_id: batchId }));
		await refreshPage();
	} catch (error) {
		message = error instanceof Error ? error.message : 'Fehlgeschlagen';
		if (button.isConnected) button.disabled = false;
	}
	const line = document.querySelector('[data-last-bulk]');
	const result = line?.querySelector('[data-last-bulk-result]');
	if (line && result) {
		result.textContent = message;
		line.hidden = false;
	}
}

function openBulk() {
	const bar = selectionBar();
	const state = currentSelection();
	if (!bar) return;
	const total = Number(bar.dataset.total) || 0;
	const target = state.allMatching
		? { filter: JSON.parse(bar.dataset.filter ?? '{}') }
		: { ids: [...state.selected] };
	const count = state.allMatching ? total : state.selected.length;
	const sheet = openSheet('[data-bulk-editor-template]');
	if (!sheet) return;
	sheet.dataset.target = JSON.stringify(target);
	const heading = sheet.querySelector('[data-bulk-heading]');
	if (heading) heading.textContent = bulkHeading(count);
	bulkResults.set(sheet, {});
	renderBulk(sheet);
}

// --- events ---

// Capture phase: a pick button sits inside a tile link, which htmx would otherwise follow.
document.addEventListener(
	'click',
	(event) => {
		const button = elementOf(event)?.closest('[data-pick]');
		if (!button) return;
		event.preventDefault();
		event.stopPropagation();
		const state = currentSelection();
		Object.assign(state, pick(state, visibleOrder(), Number(button.dataset.pick), event.shiftKey));
		renderSelection();
	},
	true
);

document.addEventListener('click', (event) => {
	const target = elementOf(event);
	if (!target) return;

	if (target.closest('[data-editor-open]')) {
		const sheet = openSheet('[data-metadata-editor-template]');
		if (sheet) renderEditor(sheet);
		return;
	}
	if (target.closest('[data-selection-clear]')) return clearSelection();
	if (target.closest('[data-select-all]')) {
		currentSelection().allMatching = true;
		return renderSelection();
	}
	if (target.closest('[data-selection-edit]')) return openBulk();
	const undoLast = target.closest('[data-last-bulk-undo]');
	if (undoLast) return void undoLastBulk(undoLast);

	const sheet = target.closest('[data-sheet]');
	if (!sheet) return;
	if (target.closest('[data-sheet-close]')) {
		event.preventDefault();
		sheet.remove();
		return;
	}
	const lock = target.closest('[data-lock]');
	if (lock) {
		event.preventDefault();
		toggleLock(sheet, lock);
		return;
	}
	if (target.closest('[data-editor-save]')) return void saveEditor(sheet);
	if (target.closest('[data-bulk-preview-button]')) return void runBulk(sheet, true);
	if (target.closest('[data-bulk-apply]')) return void runBulk(sheet, false);
	if (target.closest('[data-bulk-undo]')) return void undoBulk(sheet);
});

function onFormChange(event) {
	const sheet = elementOf(event)?.closest('[data-sheet]');
	if (!sheet) return;
	if (sheet.matches('[data-metadata-editor]')) renderEditor(sheet);
	else invalidatePreview(sheet);
}
document.addEventListener('input', onFormChange);
document.addEventListener('change', onFormChange);

// Escape closes an open sheet before anything else hears it.
document.addEventListener(
	'keydown',
	(event) => {
		if (event.key !== 'Escape') return;
		const sheet = document.querySelector('[data-sheet]');
		if (!sheet) return;
		event.preventDefault();
		event.stopImmediatePropagation();
		sheet.remove();
	},
	true
);

// Leaving the page closes an open sheet; reloading it underneath (after saving) does not.
document.addEventListener('htmx:beforeRequest', (event) => {
	if (event.detail?.boosted) closeSheets();
});
document.addEventListener('htmx:historyRestore', closeSheets);
// Tiles added by the infinite scroll show the selection too.
document.addEventListener('htmx:load', renderSelection);
document.addEventListener('DOMContentLoaded', renderSelection);
