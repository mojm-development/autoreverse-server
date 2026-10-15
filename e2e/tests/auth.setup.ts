import { test as setup, expect, type Page } from '@playwright/test';
import { NONADMIN_TEST_PASSWORD, NONADMIN_TEST_USER, TEST_PASSWORD, TEST_USER } from './_fixtures';

// Legacy created the users through drizzle if absent. Here 'Oliver' is the first admin,
// bootstrapped by the server from AUTOREVERSE_ADMIN_USER/_PASSWORD, and 'NonAdmin' comes from
// e2e/seed.sql (only if absent) — run-on-jarvis.sh sets up both before the browser starts.

async function logIn(page: Page, name: string, password: string, file: string) {
	await page.goto('/login');
	await page.getByLabel('Name', { exact: false }).fill(name);
	await page.getByLabel('Passwort').fill(password);
	await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
	await expect(page).toHaveURL('/library');
	await page.context().storageState({ path: file });
}

setup('authenticate', async ({ page }) => {
	await logIn(page, TEST_USER, TEST_PASSWORD, 'playwright/.auth/user.json');
});

setup('authenticate as non-admin', async ({ page }) => {
	await logIn(page, NONADMIN_TEST_USER, NONADMIN_TEST_PASSWORD, 'playwright/.auth/nonadmin.json');
});
