import { describe, it, expect, vi, afterEach } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { tick } from 'svelte';
import BulkEditor from './BulkEditor.svelte';

function checkbox(label: string): HTMLInputElement {
	const field = Array.from(document.querySelectorAll('.field')).find((el) =>
		el.textContent!.includes(label)
	)!;
	return field.querySelector('input[type="checkbox"]')!;
}
function valueInput(label: string): HTMLInputElement {
	const field = Array.from(document.querySelectorAll('.field')).find((el) =>
		el.textContent!.includes(label)
	)!;
	return field.querySelector('input:not([type="checkbox"])')!;
}
function button(text: string): HTMLButtonElement {
	return Array.from(document.querySelectorAll('footer button')).find(
		(el) => el.textContent!.trim() === text
	) as HTMLButtonElement;
}

async function type(input: HTMLInputElement, value: string) {
	input.value = value;
	input.dispatchEvent(new Event('input', { bubbles: true }));
	await tick();
}

const PREVIEW = {
	matched: 3,
	changed: 2,
	locked_only: 1,
	truncated: false,
	batch_id: null,
	changes: [
		{ id: 1, title: 'Live in Berlin', fields: [{ field: 'artist', old: 'ACDC', new: 'AC/DC' }] },
		{ id: 2, title: 'Back in Black', fields: [{ field: 'artist', old: 'ACDC', new: 'AC/DC' }] }
	]
};

function stubFetch(responses: unknown[]) {
	const calls: { url: string; body: Record<string, unknown> }[] = [];
	let i = 0;
	vi.stubGlobal(
		'fetch',
		vi.fn(async (url: string, init: RequestInit) => {
			calls.push({ url, body: JSON.parse(String(init.body)) });
			return { ok: true, json: async () => responses[i++] } as Response;
		})
	);
	return calls;
}

describe('BulkEditor.svelte', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('will not apply anything that has not been previewed', async () => {
		render(BulkEditor, {
			kind: 'album',
			ids: [1, 2],
			count: 2,
			onClose: () => {},
			onApplied: () => {}
		});
		expect(button('Vorschau').disabled).toBe(true);
		expect(button('Anwenden').disabled).toBe(true);

		checkbox('Interpret').click();
		await tick();
		expect(button('Vorschau').disabled).toBe(false);
		// Still nothing to apply: the diff has not been asked for yet.
		expect(button('Anwenden').disabled).toBe(true);
	});

	it('asks for a dry run first and shows the diff it gets back', async () => {
		const calls = stubFetch([PREVIEW]);
		render(BulkEditor, {
			kind: 'album',
			ids: [1, 2, 3],
			count: 3,
			onClose: () => {},
			onApplied: () => {}
		});

		checkbox('Interpret').click();
		await tick();
		await type(valueInput('Interpret'), 'AC/DC');
		button('Vorschau').click();

		await vi.waitFor(() => expect(document.querySelector('.diff')).not.toBeNull());
		expect(calls[0].body).toMatchObject({
			ids: [1, 2, 3],
			set: { artist: 'AC/DC' },
			dry_run: true
		});
		const rows = document.querySelectorAll('.diff .row');
		expect(rows).toHaveLength(2);
		expect(rows[0].textContent).toContain('ACDC');
		expect(rows[0].textContent).toContain('AC/DC');
		// The item that already had the value is reported, not hidden.
		expect(document.querySelector('.preview-count')!.textContent).toContain('nur gesperrt');
	});

	it('applies what was previewed and offers to take it back', async () => {
		const calls = stubFetch([
			PREVIEW,
			{ ...PREVIEW, batch_id: 'batch-1' },
			{ restored: 2, skipped: 0 }
		]);
		const onApplied = vi.fn();
		render(BulkEditor, {
			kind: 'album',
			ids: [1, 2, 3],
			count: 3,
			onClose: () => {},
			onApplied
		});

		checkbox('Interpret').click();
		await tick();
		await type(valueInput('Interpret'), 'AC/DC');
		button('Vorschau').click();
		await vi.waitFor(() => expect(document.querySelector('.diff')).not.toBeNull());
		button('Anwenden').click();

		await vi.waitFor(() => expect(document.querySelector('.done')).not.toBeNull());
		expect(calls[1].body).toMatchObject({ dry_run: false });
		expect(onApplied).toHaveBeenCalled();

		document.querySelector<HTMLButtonElement>('.done .outline')!.click();
		await vi.waitFor(() => expect(document.querySelector('.undone')).not.toBeNull());
		expect(calls[2].url).toBe('/items/bulk/undo');
		expect(calls[2].body).toEqual({ batch_id: 'batch-1' });
	});

	it('sends a filter instead of ids when the whole result set is selected', async () => {
		const calls = stubFetch([PREVIEW]);
		render(BulkEditor, {
			kind: 'album',
			filter: { kind: 'album', artist: 'ACDC' },
			count: 1599,
			onClose: () => {},
			onApplied: () => {}
		});

		await type(document.querySelector<HTMLInputElement>('.replace input')!, ' (Remaster)');
		button('Vorschau').click();
		await vi.waitFor(() => expect(calls).toHaveLength(1));
		expect(calls[0].body).toMatchObject({
			filter: { kind: 'album', artist: 'ACDC' },
			replace: { field: 'title', from: ' (Remaster)', to: '', regex: false }
		});
		expect(calls[0].body.ids).toBeUndefined();
	});

	it('shows the reason when the server refuses', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn(async () => ({
				ok: false,
				json: async () => ({ detail: 'Der Filter trifft mehr als 2000 Items' })
			}))
		);
		render(BulkEditor, {
			kind: 'book',
			ids: [1],
			count: 1,
			onClose: () => {},
			onApplied: () => {}
		});
		checkbox('Autor').click();
		await tick();
		button('Vorschau').click();
		await vi.waitFor(() =>
			expect(document.querySelector('.error')!.textContent).toBe(
				'Der Filter trifft mehr als 2000 Items'
			)
		);
	});

	it('the series assistant posts to its own endpoint, books only', async () => {
		const calls = stubFetch([{ ...PREVIEW, changed: 2 }]);
		render(BulkEditor, {
			kind: 'book',
			ids: [4, 5],
			count: 2,
			onClose: () => {},
			onApplied: () => {}
		});

		const detect = Array.from(document.querySelectorAll('.series .check')).find((el) =>
			el.textContent!.includes('erkennen')
		)!;
		detect.querySelector<HTMLInputElement>('input')!.click();
		await tick();
		expect(button('Vorschau').disabled).toBe(false);
		button('Vorschau').click();

		await vi.waitFor(() => expect(calls).toHaveLength(1));
		expect(calls[0].url).toBe('/items/series');
		expect(calls[0].body).toMatchObject({ ids: [4, 5], mode: 'detect', dry_run: true });
	});

	it('assigning a series needs a name before it can be previewed', async () => {
		render(BulkEditor, {
			kind: 'book',
			ids: [4],
			count: 1,
			onClose: () => {},
			onApplied: () => {}
		});
		const assign = Array.from(document.querySelectorAll('.series .check')).find((el) =>
			el.textContent!.includes('zuweisen')
		)!;
		assign.querySelector<HTMLInputElement>('input')!.click();
		await tick();
		expect(button('Vorschau').disabled).toBe(true);

		await type(document.querySelector<HTMLInputElement>('.series-name')!, 'Perry Rhodan');
		expect(button('Vorschau').disabled).toBe(false);
	});

	it('an album never sees the series assistant', async () => {
		render(BulkEditor, {
			kind: 'album',
			ids: [1],
			count: 1,
			onClose: () => {},
			onApplied: () => {}
		});
		expect(document.querySelector('.series')).toBeNull();
	});
});
