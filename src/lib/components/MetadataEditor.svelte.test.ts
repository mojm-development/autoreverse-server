import { describe, it, expect, vi, afterEach } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { tick } from 'svelte';
import MetadataEditor from './MetadataEditor.svelte';

function fields(): HTMLInputElement[] {
	return Array.from(document.querySelectorAll<HTMLInputElement>('.field input'));
}
function labels(): string[] {
	return Array.from(document.querySelectorAll('.field .label')).map((el) =>
		el.textContent!.trim().split('\n')[0].trim()
	);
}
function saveButton(): HTMLButtonElement {
	return document.querySelector<HTMLButtonElement>('footer .primary')!;
}

/** Typing the way a person does: set the value, fire input, let Svelte settle. */
async function type(input: HTMLInputElement, value: string) {
	input.value = value;
	input.dispatchEvent(new Event('input', { bubbles: true }));
	await tick();
}

const ALBUM = {
	id: 7,
	kind: 'album',
	title: 'Iron Man 2',
	sortTitle: 'iron man 2',
	artist: 'AC/DC',
	albumArtist: null,
	year: 2010,
	lockedFields: [] as string[]
};

function stubFetch() {
	const calls: { url: string; body: unknown }[] = [];
	vi.stubGlobal(
		'fetch',
		vi.fn(async (url: string, init: RequestInit) => {
			calls.push({ url, body: JSON.parse(String(init.body)) });
			return { ok: true, json: async () => ({}) } as Response;
		})
	);
	return calls;
}

describe('MetadataEditor.svelte', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('offers the fields that belong to this kind of item', async () => {
		render(MetadataEditor, { item: ALBUM, onClose: () => {}, onSaved: () => {} });
		expect(labels()).toEqual(['Titel', 'Sortiertitel', 'Interpret', 'Album-Interpret', 'Jahr']);
		// Author and narrator belong to books, and are not offered here.
		expect(labels()).not.toContain('Autor');
	});

	it('sends only what was edited', async () => {
		const calls = stubFetch();
		const onSaved = vi.fn();
		render(MetadataEditor, { item: ALBUM, onClose: () => {}, onSaved });

		await type(fields()[0], 'Iron Man 2 (Soundtrack)');
		saveButton().click();

		await vi.waitFor(() => expect(onSaved).toHaveBeenCalled());
		expect(calls).toHaveLength(1);
		expect(calls[0].url).toBe('/items/7');
		// Untouched fields stay out of the request: everything sent gets locked.
		expect(calls[0].body).toEqual({ set: { title: 'Iron Man 2 (Soundtrack)' }, reset: [] });
	});

	it('hands a locked field back to the scanner instead of writing it again', async () => {
		const calls = stubFetch();
		const onSaved = vi.fn();
		render(MetadataEditor, {
			item: { ...ALBUM, lockedFields: ['artist'] },
			onClose: () => {},
			onSaved
		});

		const lock = document.querySelector<HTMLButtonElement>('.lock')!;
		expect(lock.textContent!.trim()).toBe('von Hand');
		lock.click();
		await tick();
		saveButton().click();

		await vi.waitFor(() => expect(onSaved).toHaveBeenCalled());
		expect(calls[0].body).toEqual({ set: {}, reset: ['artist'] });
	});

	it('saves an edited track title on its own endpoint', async () => {
		const calls = stubFetch();
		const onSaved = vi.fn();
		render(MetadataEditor, {
			item: ALBUM,
			tracks: [{ id: 22, position: 1, title: 'Track 01' }],
			onClose: () => {},
			onSaved
		});

		await type(document.querySelector<HTMLInputElement>('.track input')!, 'Shoot to Thrill');
		saveButton().click();

		await vi.waitFor(() => expect(onSaved).toHaveBeenCalled());
		expect(calls).toHaveLength(1);
		expect(calls[0].url).toBe('/tracks/22');
		expect(calls[0].body).toEqual({ set: { title: 'Shoot to Thrill' } });
	});

	it('sends a number field as a number, not as the string the input holds', async () => {
		const calls = stubFetch();
		const onSaved = vi.fn();
		render(MetadataEditor, { item: ALBUM, onClose: () => {}, onSaved });

		await type(fields()[4], '2011');
		saveButton().click();
		await vi.waitFor(() => expect(onSaved).toHaveBeenCalled());
		expect(calls[0].body).toEqual({ set: { year: 2011 }, reset: [] });
	});

	it('clears a field that is emptied rather than sending an empty string', async () => {
		const calls = stubFetch();
		const onSaved = vi.fn();
		render(MetadataEditor, { item: ALBUM, onClose: () => {}, onSaved });

		await type(fields()[2], '   ');
		saveButton().click();
		await vi.waitFor(() => expect(onSaved).toHaveBeenCalled());
		expect(calls[0].body).toEqual({ set: { artist: null }, reset: [] });
	});

	it('stays disabled until something changes, and shows what the server refused', async () => {
		expect.hasAssertions();
		vi.stubGlobal(
			'fetch',
			vi.fn(async () => ({ ok: false, json: async () => ({ detail: 'Nur für Verwalter' }) }))
		);
		render(MetadataEditor, { item: ALBUM, onClose: () => {}, onSaved: () => {} });
		expect(saveButton().disabled).toBe(true);

		await type(fields()[4], '2011');
		expect(saveButton().disabled).toBe(false);
		saveButton().click();

		await vi.waitFor(() =>
			expect(document.querySelector('.error')?.textContent).toBe('Nur für Verwalter')
		);
	});
});
