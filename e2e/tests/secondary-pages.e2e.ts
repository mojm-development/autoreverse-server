import { expect, test } from '@playwright/test';

// The album "E2E-Fixture-Album-ForArtist" by this artist is seeded by e2e/seed.sql.
const ARTIST_NAME = 'Ansa Volt';

test('every sidebar link resolves to a real page, not a 404', async ({ page }) => {
	for (const path of [
		'/library/favorites',
		'/library/artists',
		'/library/series',
		'/library/bookmarks'
	]) {
		const response = await page.goto(path);
		expect(response?.status()).toBeLessThan(400);
	}
});

test('clicking an artist on /library/artists filters the Alben page to that artist', async ({
	page
}) => {
	await page.goto('/library/artists');
	await page.getByText(ARTIST_NAME).click();
	await expect(page).toHaveURL(/\/library\/albums\?artist=/);
});
