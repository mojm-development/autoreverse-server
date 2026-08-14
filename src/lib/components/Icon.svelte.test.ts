import { page } from 'vitest/browser';
import { describe, it, expect } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Icon from './Icon.svelte';
import IconSprite from './IconSprite.svelte';

describe('Icon.svelte', () => {
	it('references the requested symbol id via <use href>', async () => {
		render(IconSprite, {});
		render(Icon, { name: 'play' });
		// SVG <use> has no accessible role/text, and this project's
		// @vitest/browser Page has no public `.locator()` for a raw CSS
		// selector (confirmed empirically) — fall back to the data-testid
		// the corrections file explicitly allows, but keep the assertion on
		// the real `href` value of the rendered <use> element.
		const svg = page.getByTestId('icon-play');
		await expect.element(svg).toBeInTheDocument();
		const use = svg.element().querySelector('use');
		expect(use?.getAttribute('href')).toBe('#icon-play');
	});
});
