import { page } from 'vitest/browser';
import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import SegmentFilter from './SegmentFilter.svelte';

describe('SegmentFilter.svelte', () => {
	it('marks the active option and calls onChange when another is clicked', async () => {
		const onChange = vi.fn();
		render(SegmentFilter, {
			options: [
				{ label: 'Alle', value: 'all' },
				{ label: 'Musik', value: 'music' }
			],
			value: 'all',
			onChange
		});
		await expect.element(page.getByText('Alle')).toHaveAttribute('aria-current', 'true');
		await page.getByText('Musik').click();
		expect(onChange).toHaveBeenCalledWith('music');
	});
});
