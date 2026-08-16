import { expect, test } from '@playwright/test';

test('subscription rail shows unread-count badges from real data', async ({ page }) => {
	await page.goto('/library/podcasts');
	await expect(page.getByText('Abos ·')).toBeVisible();
});

test.describe('as a non-admin', () => {
	test.use({ storageState: 'playwright/.auth/nonadmin.json' });

	test('the add-feed prompt is hidden for a non-admin user', async ({ page }) => {
		await page.goto('/library/podcasts');
		await expect(page.getByText('Feed-URL hinzufügen')).not.toBeVisible();
	});
});
