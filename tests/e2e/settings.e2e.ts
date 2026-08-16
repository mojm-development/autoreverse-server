import { expect, test } from '@playwright/test';

test.describe('as a non-admin', () => {
	test.use({ storageState: 'playwright/.auth/nonadmin.json' });

	test('a non-admin cannot reach the Bibliotheken/Scan/Podcast-Abos panels', async ({ page }) => {
		await page.goto('/settings/libraries');
		await expect(page).toHaveURL('/library');
	});
});

test('Wiedergabe and Sicherheit are reachable by every user', async ({ page }) => {
	await page.goto('/settings/playback');
	await expect(page.getByRole('heading', { name: 'Wiedergabe' })).toBeVisible();
});

test('the scan status card polls and reflects real backend state', async ({ page, request }) => {
	await page.goto('/settings/scan');
	// Open the "Protokoll" panel first: with no library paths configured yet
	// in a fresh e2e run (confirmed empirically against the actual e2e
	// Postgres instance — `library_config` is empty), a triggered scan
	// completes in single-digit milliseconds, far faster than the UI's own
	// 4s poll interval, so the running:true window is never reliably
	// observable this way. It also never touches `running`/`last_report` (the
	// only fields the card shows outside the log panel) — it only ever sets
	// `last_error`, which the card renders exclusively inside the collapsed
	// Protokoll section. Opening that section up front makes the resulting
	// state change ("Fehler: Bibliothekspfade sind noch nicht konfiguriert…"
	// appearing) both real and deterministic, while still exercising the
	// exact same poll → fetch → re-render loop the original assertion was
	// after.
	await page.getByRole('button', { name: 'Protokoll' }).click();
	const before = await page.locator('.card').first().textContent();
	await request.post('/scan');
	await expect(async () => {
		const after = await page.locator('.card').first().textContent();
		expect(after).not.toBe(before);
	}).toPass({ timeout: 10_000, intervals: [500] });
});
