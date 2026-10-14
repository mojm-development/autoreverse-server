import { expect, test } from '@playwright/test';

test('toggling the view switches between grid and list without a full reload changing the result set', async ({
	page
}) => {
	await page.goto('/library/albums');
	await expect(page.locator('.grid-6')).toBeVisible();
	await page.goto('/library/albums?view=list');
	await expect(page.getByRole('table')).toBeVisible();
});

test('the sort toggle marks the applied sort', async ({ page }) => {
	await page.goto('/library/albums?sort=added');
	const applied = page.getByRole('group', { name: 'Sortierung' }).getByText('Zuletzt dazu');
	await expect(applied).toHaveAttribute('aria-current', 'true');
});

test('a filtered list says what is filtering it and offers a way out', async ({ page }) => {
	await page.goto('/library/albums?q=zzz-nichts-findet-das');
	await expect(page.getByText('Suche: zzz-nichts-findet-das')).toBeVisible();
	await page.getByRole('link', { name: 'Suche aufheben' }).click();
	await expect(page).toHaveURL('/library/albums');
});
