import { expect, test } from '@playwright/test';
import { itemId } from './_fixtures';

// The album and its two tracks are seeded by e2e/seed.sql.
const FIXTURE_TITLE = 'E2E-Fixture-Album-Detail';

test('clicking Abspielen starts playback and the mini-player bar appears', async ({ page }) => {
	const albumId = await itemId(page.request, FIXTURE_TITLE);
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Abspielen', exact: true }).click();
	await expect(page.getByTestId('mini-player')).toBeVisible();
});

test('the currently-playing track row is visually distinguished', async ({ page }) => {
	const albumId = await itemId(page.request, FIXTURE_TITLE);
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Abspielen', exact: true }).click();
	await expect(page.locator('[aria-current="true"][role="row"]')).toBeVisible();
});
