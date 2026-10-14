import { page } from 'vitest/browser';
import { describe, it, expect } from 'vitest';
import { render } from 'vitest-browser-svelte';
import ContinueCard from './ContinueCard.svelte';

describe('ContinueCard.svelte', () => {
	it('renders title, subtitle, duration, and links to href', async () => {
		render(ContinueCard, {
			kind: 'book',
			coverUrl: null,
			title: 'Der Report',
			subtitle: 'Kapitel 3',
			progressPercent: 40,
			durationLabel: '2:15 Std',
			href: '/library/books/1'
		});
		await expect.element(page.getByText('Der Report')).toBeInTheDocument();
		await expect.element(page.getByText('Kapitel 3')).toBeInTheDocument();
		await expect.element(page.getByText('2:15 Std')).toBeInTheDocument();
		await expect.element(page.getByRole('link')).toHaveAttribute('href', '/library/books/1');
	});
});
