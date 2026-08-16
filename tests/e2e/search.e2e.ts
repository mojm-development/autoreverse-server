import { expect, test } from '@playwright/test';

test('searching updates the URL query and shows grouped result counts', async ({ page }) => {
	await page.goto('/library/search');
	await page.getByRole('searchbox').fill('eschbach');
	await page.getByRole('searchbox').press('Enter');
	await expect(page).toHaveURL(/q=eschbach/);
	await expect(page.getByText(/Treffer in \d+ Items/)).toBeVisible();
});
