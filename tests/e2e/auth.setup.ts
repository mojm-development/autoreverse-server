import { test as setup, expect } from '@playwright/test';
import { eq } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import { users } from '../../src/lib/server/db/schema';
import { createUser } from '../../src/lib/server/auth/passwords';

export const TEST_USER = 'Oliver';
export const TEST_PASSWORD = 'hunter2hunter2';
const AUTH_FILE = 'playwright/.auth/user.json';

setup('authenticate', async ({ page }) => {
	// Idempotent: the shared dev Postgres used by e2e runs may already have
	// users from earlier manual `pnpm dev`/review sessions — don't assume a
	// clean DB, don't assume `ensureFirstAdmin` fired with this exact name.
	const [existing] = await db.select({ id: users.id }).from(users).where(eq(users.name, TEST_USER));
	if (!existing) {
		await createUser(db, TEST_USER, TEST_PASSWORD, true);
	}

	await page.goto('/login');
	await page.getByLabel('Name', { exact: false }).fill(TEST_USER);
	await page.getByLabel('Passwort').fill(TEST_PASSWORD);
	await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
	await expect(page).toHaveURL('/library');
	await page.context().storageState({ path: AUTH_FILE });
});
