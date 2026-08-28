import { page } from 'vitest/browser';
import { describe, it, expect } from 'vitest';
import { createRawSnippet } from 'svelte';
import { render } from 'vitest-browser-svelte';
import ListRow from './ListRow.svelte';

// ListRow's only prop besides `href` is a required `children` snippet, which
// is normally authored via {#snippet} in .svelte markup rather than passed
// as a bare function from a .test.ts file. `createRawSnippet` (Svelte's own
// testing escape hatch) makes it possible to construct one directly, so a
// real test is not "genuinely awkward" here after all.
function textSnippet(text: string) {
	return createRawSnippet(() => ({
		render: () => `<span>${text}</span>`
	}));
}

describe('ListRow.svelte', () => {
	it('renders as an anchor with the given href', async () => {
		render(ListRow, { href: '/library/albums/1', children: textSnippet('Nordlicht') });
		const row = page.getByRole('row');
		await expect.element(row).toHaveAttribute('href', '/library/albums/1');
		await expect.element(page.getByText('Nordlicht')).toBeInTheDocument();
	});

	it('renders as a plain div when no href is given', async () => {
		render(ListRow, { children: textSnippet('Ansa Volt') });
		const row = page.getByRole('row');
		await expect.element(row).not.toHaveAttribute('href');
	});

	it('activates on click when onclick is given', async () => {
		let clicks = 0;
		render(ListRow, {
			onclick: () => (clicks += 1),
			label: 'Punk ist... abspielen',
			children: textSnippet('Punk ist...')
		});
		await page.getByLabelText('Punk ist... abspielen').click();
		expect(clicks).toBe(1);
	});

	it('keeps the activator out of the button so nested controls stay valid', async () => {
		// A <button> may not contain interactive content: wrapping the row in
		// one makes the parser close it at the first nested button, which
		// collapses the layout. The activator must therefore be a sibling of
		// the row content, not its ancestor.
		render(ListRow, {
			onclick: () => {},
			label: 'Rebell abspielen',
			children: textSnippet('Rebell')
		});
		const activator = page.getByLabelText('Rebell abspielen');
		await expect.element(activator).toBeInTheDocument();
		await expect.element(page.getByText('Rebell')).toBeInTheDocument();
		expect(document.querySelector('button button')).toBeNull();
	});
});
