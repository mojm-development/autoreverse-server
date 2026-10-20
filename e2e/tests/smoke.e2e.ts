import { expect, test } from '@playwright/test';

// The fixture library lives on the machine running the server, not in the browser VM, so its
// paths come in from run-on-jarvis.sh. The database is recreated for every run there, so both
// fixture folders are always "neu" (legacy deleted their items through drizzle first).
const BOOKS_DIR = process.env.E2E_BOOKS_DIR!;
const MUSIC_DIR = process.env.E2E_MUSIC_DIR!;

// Runs without a pre-authenticated session — the whole point of this test is
// proving the login cookie genuinely round-trips through a real browser. Left
// to the default `authenticated` project it would inherit
// storageState: 'playwright/.auth/user.json' and start already logged in,
// silently skipping the thing it's meant to test.
test.use({ storageState: { cookies: [], origins: [] } });

test('full journey: login, scan, browse, play, progress persists across reload', async ({
	page
}) => {
	// The default 30s test timeout is too tight alongside a 30s expect timeout
	// further down (waiting on the scan to actually finish against the real
	// filesystem/DB) plus everything before and after it in this one long
	// journey — extend it rather than touching playwright.config.ts's global
	// default, which is shared by every other e2e spec.
	test.setTimeout(60_000);

	expect(BOOKS_DIR && MUSIC_DIR, 'E2E_BOOKS_DIR/E2E_MUSIC_DIR fehlen').toBeTruthy();

	await page.goto('/login');
	await page.getByLabel('Name', { exact: false }).fill('Oliver');
	await page.getByLabel('Passwort').fill('hunter2hunter2');
	await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
	await expect(page).toHaveURL('/library');

	// Configure library paths through the real admin UI (Bibliotheken panel),
	// not a backdoor DB write — exercises Task 39's own save flow for real.
	await page.goto('/settings/libraries');
	await page.getByLabel('Bücher-Pfad', { exact: false }).fill(BOOKS_DIR);
	await page.getByLabel('Musik-Pfad', { exact: false }).fill(MUSIC_DIR);
	// The form posts through htmx: wait for the save itself, or the goto below aborts it.
	const saved = page.waitForResponse((r) => r.url().includes('savePaths') && r.ok());
	await page.getByRole('button', { name: 'Speichern' }).click();
	await saved;

	await page.goto('/settings/scan');
	await page.getByRole('button', { name: 'Scan starten' }).click();
	await expect(page.getByText('Scan läuft')).toBeVisible();
	await expect(page.locator('#scan-card [data-count="new"] dd')).toHaveText('2', { timeout: 30_000 });

	await page.goto('/library/books');
	await page.getByText('Buch', { exact: true }).click();
	await expect(page).toHaveURL(/\/library\/books\/\d+/);
	await page.getByRole('button', { name: 'Abspielen', exact: true }).click();
	await expect(page.getByTestId('mini-player')).toBeVisible();

	await page.waitForTimeout(3_000); // let a little real position accumulate
	await page.reload();
	await expect(page.getByText(/Weiter ab/)).toBeVisible();
});
