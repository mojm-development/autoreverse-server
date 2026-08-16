import { expect, test } from '@playwright/test';

test('shows the unencrypted-connection warning and library stats', async ({ page }) => {
	await page.goto('/login');
	await expect(page.getByText('Diese Verbindung ist unverschlüsselt.')).toBeVisible();
	await expect(page.getByRole('heading', { name: 'Ein Server. Alles zum Hören.' })).toBeVisible();
});

test('the password show/hide toggle actually toggles the input type', async ({ page }) => {
	await page.goto('/login');
	const password = page.getByLabel('Passwort');
	await expect(password).toHaveAttribute('type', 'password');
	await page.getByRole('button', { name: 'zeigen' }).click();
	await expect(password).toHaveAttribute('type', 'text');
});

test('no OpenID button is shown (no backend support exists)', async ({ page }) => {
	await page.goto('/login');
	await expect(page.getByRole('button', { name: 'Mit OpenID anmelden' })).toHaveCount(0);
});
