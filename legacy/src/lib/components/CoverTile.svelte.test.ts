import { page } from 'vitest/browser';
import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import CoverTile from './CoverTile.svelte';

describe('CoverTile.svelte', () => {
	it('renders an <img> when coverUrl is set', async () => {
		render(CoverTile, {
			kind: 'album',
			coverUrl: '/items/1/cover',
			title: 'Nordlicht',
			subtitle: 'Ansa Volt'
		});
		// CoverTile sets alt="" (decorative cover art), which per ARIA/HTML-AAM
		// gives the <img> an implicit "presentation" role rather than "img" —
		// so getByRole('img') can never match it (confirmed empirically; `page`
		// also has no public `.locator()` escape hatch in this project's
		// @vitest/browser version). getByAltText('') does match it directly,
		// per the same "adapt the selection mechanism, keep the src assertion"
		// allowance the corrections file grants for Icon's <use> element.
		const img = page.getByAltText('');
		await expect.element(img).toHaveAttribute('src', '/items/1/cover');
	});

	it('renders the striped placeholder when coverUrl is null', async () => {
		render(CoverTile, { kind: 'book', coverUrl: null, title: 'X', subtitle: 'Y' });
		await expect.element(page.getByAltText('')).not.toBeInTheDocument();
	});

	it('carries a play button only when one is offered, and playing does not follow the link', async () => {
		const onPlay = vi.fn();
		const { unmount } = render(CoverTile, {
			kind: 'album',
			coverUrl: null,
			title: 'Iron Man 2',
			subtitle: 'AC/DC'
		});
		expect(document.querySelector('.play')).toBeNull();
		unmount();

		render(CoverTile, {
			kind: 'album',
			coverUrl: null,
			title: 'Iron Man 2',
			subtitle: 'AC/DC',
			playLabel: 'Iron Man 2 abspielen',
			onPlay
		});
		const play = document.querySelector<HTMLButtonElement>('.play')!;
		expect(play.getAttribute('aria-label')).toBe('Iron Man 2 abspielen');

		// The tile sits inside a link to the detail page; the button must not navigate.
		const event = new MouseEvent('click', { bubbles: true, cancelable: true });
		play.dispatchEvent(event);
		expect(onPlay).toHaveBeenCalled();
		expect(event.defaultPrevented).toBe(true);
	});
});
