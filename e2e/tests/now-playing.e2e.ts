import { expect, test } from '@playwright/test';
import { itemId } from './_fixtures';

// Legacy had no in-app link to the fullscreen player and injected an <a> for SvelteKit's router to
// pick up. Here the mini-player's "Großer Player" link is that way in: it sits inside the htmx-boosted
// part of the layout, so following it swaps only #page and the player keeps its state — a
// page.goto() (or an <a> outside the boosted root) would be a full load and reset it.

test('the fullscreen player has no sidebar and no mini-player bar', async ({ page }) => {
	await page.goto('/library/1/player');
	await expect(page.locator('nav.sidebar')).not.toBeVisible();
	// Not `.bar` — the visualizer's columns carry that class too.
	await expect(page.getByTestId('mini-player')).not.toBeVisible();
});

test('closing returns to the previous library page with the mini-player bar restored', async ({
	page
}) => {
	const bookId = await itemId(page.request, 'E2E-Fixture-NowPlaying-Book');
	await page.goto(`/library/books/${bookId}`);
	await page.getByRole('button', { name: 'Abspielen', exact: true }).click();
	await expect(page.getByTestId('mini-player')).toBeVisible();

	await page.getByRole('link', { name: 'Großer Player' }).click();
	await expect(page).toHaveURL(`/library/${bookId}/player`);
	await page.getByRole('button', { name: 'Zurück', exact: true }).click();

	await expect(page).toHaveURL(`/library/books/${bookId}`);
	await expect(page.getByTestId('mini-player')).toBeVisible();
});
