import { describe, it, expect } from 'vitest';
import { render } from 'vitest-browser-svelte';
import PageTitle from './PageTitle.svelte';

describe('PageTitle.svelte', () => {
	it('puts the page name before the brand', async () => {
		render(PageTitle, { title: 'Lesezeichen' });
		expect(document.title).toBe('Lesezeichen · Autoreverse');
	});

	it('falls back to the bare brand without a name', async () => {
		render(PageTitle, {});
		expect(document.title).toBe('Autoreverse');
	});
});
