import { expect, test } from '@playwright/test';

// Scheibe 5 (Suche): /library/search gibt es im Rust-Server noch nicht. Mit der Scheibe wandert
// das Fixture-Album „Zzz-Test-Album-9182-NoMatch“ mit dem Track „Zzz-Test-Track-Only-9182“ nach
// e2e/seed.sql; legacy legte es per drizzle an.
test.fixme();

test('searching updates the URL query and shows grouped result counts', async ({ page }) => {
	await page.goto('/library/search');
	await page.getByRole('searchbox').fill('eschbach');
	await page.getByRole('searchbox').press('Enter');
	await expect(page).toHaveURL(/q=eschbach/);
	await expect(page.getByText(/Treffer in \d+ Items/)).toBeVisible();
});

test('track-only search results are visible when no albums match', async ({ page }) => {
	// Seeded by e2e/seed.sql (from Scheibe 5 on)
	const testTrackTitle = 'Zzz-Test-Track-Only-9182';

	// Search for the track by its distinctive title
	await page.goto('/library/search');
	await page.getByRole('searchbox').fill(testTrackTitle);
	await page.getByRole('searchbox').press('Enter');

	// Assert the track title is visible in the results
	await expect(page.getByText(testTrackTitle)).toBeVisible();
});
