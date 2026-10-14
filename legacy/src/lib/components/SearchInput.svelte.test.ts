import { page } from 'vitest/browser';
import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import SearchInput from './SearchInput.svelte';

describe('SearchInput.svelte', () => {
	it('calls oninput with the typed value', async () => {
		const oninput = vi.fn();
		render(SearchInput, { value: '', placeholder: 'Suchen…', oninput });
		await page.getByPlaceholder('Suchen…').fill('Nordlicht');
		expect(oninput).toHaveBeenCalledWith('Nordlicht');
	});
});
