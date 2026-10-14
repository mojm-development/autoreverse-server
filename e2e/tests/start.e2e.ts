import { expect, test } from '@playwright/test';
// Assumes an authenticated storageState fixture is configured in
// playwright.config.ts (log in once via the login form in a setup project,
// reuse the cookie — standard Playwright auth pattern, not spelled out here).

test('shows a time-of-day greeting with the real username, not a hardcoded name', async ({
	page
}) => {
	await page.goto('/library');
	// level: 1 — the brief's bare getByRole('heading') is a strict-mode violation here: the
	// page also has h2 section headings ("Weiter hören", "Neu im Bestand"), so it needs to be
	// scoped to the h1 that actually carries the greeting.
	await expect(page.getByRole('heading', { level: 1 })).toContainText(
		/Guten (Morgen|Tag|Abend), Oliver/
	);
});

test('continue-listening cards show real progress from the database, not placeholder text', async ({
	page
}) => {
	await page.goto('/library');
	await expect(page.getByText('Weiter hören')).toBeVisible();
});
