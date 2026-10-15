import { expect, test } from '@playwright/test';

// Same credentials as auth.setup.ts's TEST_USER/TEST_PASSWORD (duplicated, not
// imported: Playwright disallows importing one test file from another).
const TEST_USER = 'Oliver';
const TEST_PASSWORD = 'hunter2hunter2';

// Deliberately does NOT reuse the shared `authenticated` project's storageState:
// that file's token is shared by every other spec in this run, and logging out
// revokes the token server-side — reusing it here would silently log every
// other in-flight/later test out of its session too. Log in fresh instead, so
// only this test's own throwaway token is ever revoked.
test.use({ storageState: { cookies: [], origins: [] } });

test('logging out via the sidebar clears the session and redirects to /login', async ({ page }) => {
	await page.goto('/login');
	await page.getByLabel('Name', { exact: false }).fill(TEST_USER);
	await page.getByLabel('Passwort').fill(TEST_PASSWORD);
	await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
	await expect(page).toHaveURL('/library');

	await page.getByRole('button', { name: 'Abmelden' }).click();
	await expect(page).toHaveURL('/login');

	// The session must actually be gone server-side, not just the UI hidden.
	await page.goto('/library');
	await expect(page).toHaveURL('/login');
});
