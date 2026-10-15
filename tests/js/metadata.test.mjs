// The metadata editors' decisions (legacy MetadataEditor.svelte.test.ts and
// BulkEditor.svelte.test.ts, against static/js/metadata/logic.js; the DOM wiring is covered by
// the e2e spec metadata.e2e.ts). Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
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
	trackPatches,
	toWire,
	undoneText
} from '../../static/js/metadata/logic.js';

// The album of the legacy component test, as the form holds it.
function albumFields(overrides = {}) {
	const fields = [
		{ wire: 'title', type: 'text', initial: 'Iron Man 2' },
		{ wire: 'sort_title', type: 'text', initial: 'iron man 2' },
		{ wire: 'artist', type: 'text', initial: 'AC/DC' },
		{ wire: 'album_artist', type: 'text', initial: '' },
		{ wire: 'year', type: 'int', initial: '2010' }
	];
	return fields.map((f) => ({ ...f, value: overrides[f.wire] ?? f.initial }));
}

test('sends only what was edited', () => {
	assert.deepEqual(itemPatch(albumFields({ title: 'Iron Man 2 (Soundtrack)' }), []), {
		set: { title: 'Iron Man 2 (Soundtrack)' },
		reset: []
	});
});

test('hands a locked field back to the scanner instead of writing it again', () => {
	assert.deepEqual(itemPatch(albumFields(), ['artist']), { set: {}, reset: ['artist'] });
	// Even an edited value is not sent while the field is being handed back.
	assert.deepEqual(itemPatch(albumFields({ artist: 'X' }), ['artist']), {
		set: {},
		reset: ['artist']
	});
});

test('saves an edited track title on its own endpoint', () => {
	assert.deepEqual(
		trackPatches([
			{ id: 22, initial: 'Track 01', value: 'Shoot to Thrill' },
			{ id: 23, initial: 'Track 02', value: 'Track 02' }
		]),
		[{ id: 22, body: { set: { title: 'Shoot to Thrill' } } }]
	);
	// Nothing about the item itself changed: no item request.
	assert.equal(itemPatch(albumFields(), []), null);
});

test('sends a number field as a number, not as the string the input holds', () => {
	assert.deepEqual(itemPatch(albumFields({ year: '2011' }), []), {
		set: { year: 2011 },
		reset: []
	});
	assert.equal(toWire('float', '2,5'), 2.5);
});

test('clears a field that is emptied rather than sending an empty string', () => {
	assert.deepEqual(itemPatch(albumFields({ artist: '   ' }), []), {
		set: { artist: null },
		reset: []
	});
});

test('stays disabled until something changes', () => {
	assert.equal(editorDirty(albumFields(), [], []), false);
	assert.equal(editorDirty(albumFields({ year: '2011' }), [], []), true);
	assert.equal(editorDirty(albumFields(), ['artist'], []), true);
	assert.equal(editorDirty(albumFields(), [], [{ id: 1, initial: 'a', value: 'b' }]), true);
});

function bulkState(overrides = {}) {
	return {
		target: { ids: [1, 2, 3] },
		fields: [
			{ wire: 'title', type: 'text', active: false, value: '' },
			{ wire: 'artist', type: 'text', active: false, value: '' },
			{ wire: 'year', type: 'int', active: false, value: '' }
		],
		replace: { field: 'title', from: '', to: '', regex: false },
		seriesMode: 'off',
		seriesName: '',
		seriesStart: '1',
		...overrides
	};
}

const PREVIEW = {
	matched: 3,
	changed: 2,
	locked_only: 1,
	truncated: false,
	batch_id: null,
	changes: []
};

test('will not apply anything that has not been previewed', () => {
	assert.equal(bulkHasWork(bulkState()), false);
	const state = bulkState();
	state.fields[1].active = true;
	assert.equal(bulkHasWork(state), true);
	// Still nothing to apply: the diff has not been asked for yet.
	assert.equal(canApply(null, false), false);
	assert.equal(canApply(PREVIEW, false), true);
	assert.equal(canApply(PREVIEW, true), false);
	assert.equal(canApply({ ...PREVIEW, changed: 0, locked_only: 0 }, false), false);
});

test('asks for a dry run first, then applies', () => {
	const state = bulkState();
	state.fields[1] = { wire: 'artist', type: 'text', active: true, value: 'AC/DC' };
	assert.deepEqual(bulkBody(state, true), {
		ids: [1, 2, 3],
		set: { artist: 'AC/DC' },
		dry_run: true
	});
	assert.equal(bulkBody(state, false).dry_run, false);
	assert.equal(bulkEndpoint(state), '/items/bulk');
	// An active field left blank clears it.
	state.fields[2] = { wire: 'year', type: 'int', active: true, value: '' };
	assert.equal(bulkBody(state, true).set.year, null);
});

test('reports the item that already had the value, not hides it', () => {
	assert.equal(previewText(PREVIEW), '2 von 3 werden geändert, 1 nur gesperrt.');
	assert.equal(
		previewText({ ...PREVIEW, changed: 0, locked_only: 2 }),
		'Nichts zu ändern — 2 Einträge haben den Wert schon.'
	);
	assert.equal(previewText({ ...PREVIEW, changed: 0, locked_only: 0 }), 'Nichts zu ändern.');
	assert.equal(appliedText({ changed: 1, locked_only: 0 }), '1 Eintrag geändert');
	assert.equal(undoneText({ restored: 2, skipped: 0 }), 'Zurückgenommen: 2 Felder');
	assert.equal(
		undoneText({ restored: 1, skipped: 1 }),
		'Zurückgenommen: 1 Feld, 1 übersprungen (inzwischen anders geändert)'
	);
	assert.equal(bulkHeading(1), '1 Eintrag bearbeiten');
	assert.equal(bulkHeading(1599), '1599 Einträge bearbeiten');
});

test('sends a filter instead of ids when the whole result set is selected', () => {
	const state = bulkState({
		target: { filter: { kind: 'album', artist: 'ACDC' } },
		replace: { field: 'title', from: ' (Remaster)', to: '', regex: false }
	});
	assert.equal(bulkHasWork(state), true);
	const body = bulkBody(state, true);
	assert.deepEqual(body, {
		filter: { kind: 'album', artist: 'ACDC' },
		set: {},
		replace: { field: 'title', from: ' (Remaster)', to: '', regex: false },
		dry_run: true
	});
	assert.equal(body.ids, undefined);
});

test('the series assistant posts to its own endpoint', () => {
	const detect = bulkState({ target: { ids: [4, 5] }, seriesMode: 'detect' });
	assert.equal(bulkHasWork(detect), true);
	assert.equal(bulkEndpoint(detect), '/items/series');
	assert.deepEqual(bulkBody(detect, true), { ids: [4, 5], mode: 'detect', dry_run: true });
});

test('assigning a series needs a name before it can be previewed', () => {
	const assign = bulkState({ target: { ids: [4] }, seriesMode: 'assign' });
	assert.equal(bulkHasWork(assign), false);
	assign.seriesName = 'Perry Rhodan ';
	assign.seriesStart = '5';
	assert.equal(bulkHasWork(assign), true);
	assert.deepEqual(bulkBody(assign, true), {
		ids: [4],
		mode: 'assign',
		series: 'Perry Rhodan',
		start: 5,
		dry_run: true
	});
	assign.seriesStart = '';
	assert.equal(bulkBody(assign, true).start, null);
});

test('a click toggles a tile, a shift-click adds the range', () => {
	const order = [10, 11, 12, 13];
	let state = { selected: [], lastPicked: null, allMatching: false };
	state = pick(state, order, 11, false);
	assert.deepEqual(state.selected, [11]);
	state = pick(state, order, 13, true);
	assert.deepEqual(state.selected, [11, 12, 13]);
	state = pick(state, order, 12, false);
	assert.deepEqual(state.selected, [11, 13]);
	// Picking a single tile leaves "all matching".
	assert.equal(pick({ ...state, allMatching: true }, order, 10, false).allMatching, false);
});

test('the selection bar counts, and offers all only where the list can be filtered', () => {
	assert.equal(selectionText(2, 150, false), '2 ausgewählt');
	assert.equal(selectionText(2, 150, true), 'Alle 150 ausgewählt');
	assert.equal(offerSelectAll(2, 150, false, true), true);
	assert.equal(offerSelectAll(2, 150, true, true), false);
	assert.equal(offerSelectAll(2, 2, false, true), false);
	assert.equal(offerSelectAll(2, 150, false, false), false);
});
