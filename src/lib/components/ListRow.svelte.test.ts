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
});
