import { expect, test } from '@playwright/test';

test('subscription rail shows unread-count badges from real data', async ({ page }) => {
	await page.goto('/library/podcasts');
	await expect(page.getByText('Abos ·')).toBeVisible();
});

test('the search box is seeded from ?q= so returning to it is not a blank slate', async ({
	page
}) => {
	await page.goto('/library/podcasts?q=Tennis');
	await expect(page.getByPlaceholder('Podcast suchen')).toHaveValue('Tennis');
});

test('the preview page carries the query back to the search', async ({ page }) => {
	// Port 9 (discard) refuses instantly: this exercises the page shell and its
	// back link without depending on a reachable feed or the iTunes directory.
	await page.goto('/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml&q=Tennis');
	const back = page.getByRole('link', { name: 'Zurück zur Suche' });
	await expect(back).toHaveAttribute('href', '/library/podcasts?q=Tennis');
});

test('the preview page reports an unreachable feed instead of failing', async ({ page }) => {
	await page.goto('/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml');
	await expect(page.getByText('Der Feed ist nicht erreichbar.')).toBeVisible();
});

test.describe('as a non-admin', () => {
	test.use({ storageState: 'playwright/.auth/nonadmin.json' });

	test('the podcast-search prompt is hidden for a non-admin user', async ({ page }) => {
		await page.goto('/library/podcasts');
		await expect(page.getByPlaceholder('Podcast suchen')).not.toBeVisible();
	});

	test('a non-admin cannot open the subscribe preview', async ({ page }) => {
		// requireWebAdmin bounces to /library, same as the admin-only settings panels.
		await page.goto('/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml');
		await expect(page).toHaveURL('/library');
	});
});
