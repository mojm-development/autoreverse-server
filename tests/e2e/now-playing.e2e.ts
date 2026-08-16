import { expect, test } from '@playwright/test';

test('the fullscreen player has no sidebar and no mini-player bar', async ({ page }) => {
	await page.goto('/library/1/player');
	await expect(page.locator('nav.sidebar')).not.toBeVisible();
	await expect(page.locator('.bar')).not.toBeVisible();
});

test('closing returns to the previous library page with the mini-player bar restored', async ({
	page
}) => {
	await page.goto('/library/books/1');
	await page.getByRole('button', { name: 'Abspielen' }).click();
	await page.goto('/library/1/player');
	await page.getByRole('button', { name: 'Schließen' }).click();
	await expect(page.locator('.bar')).toBeVisible();
});
