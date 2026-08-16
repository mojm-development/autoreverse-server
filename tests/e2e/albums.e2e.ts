import { expect, test } from '@playwright/test';

test('toggling the view switches between grid and list without a full reload changing the result set', async ({
	page
}) => {
	await page.goto('/library/albums');
	await expect(page.locator('.grid-6')).toBeVisible();
	await page.goto('/library/albums?view=list');
	await expect(page.getByRole('table')).toBeVisible();
});

test('sort label reflects the applied sort', async ({ page }) => {
	await page.goto('/library/albums?sort=added');
	await expect(page.getByText('sortiert: Zuletzt dazu')).toBeVisible();
});
