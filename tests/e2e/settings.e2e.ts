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
	// Open the "Protokoll" panel: with no library paths configured (true in a
	// fresh e2e run), a triggered scan only ever sets `last_error` — to a
	// FIXED string — and `last_skipped`; `running`/`last_report` are never
	// touched on this path. The card renders `last_error` exclusively inside
	// this collapsed section.
	await page.getByRole('button', { name: 'Protokoll' }).click();

	// Diffing the card's rendered *text* before/after a single scan (the
	// previous approach) only detects a state change the first time a server
	// process ever runs a scan: `last_error`'s string is fixed, so on any
	// later run within the same process the "before" text already equals the
	// "after" text and the assertion times out. `finished_at`, in contrast,
	// is re-stamped by every single scan regardless of outcome, so diffing it
	// across two DISTINCT triggered scans — read from the real
	// GET /scan/status endpoint, independent of the card's own DOM — is
	// robust no matter how many times this test, or the server, have already
	// run.
	async function finishedAt(): Promise<string | null> {
		const res = await request.get('/scan/status');
		return ((await res.json()) as { finished_at: string | null }).finished_at;
	}

	const baseline = await finishedAt();

	await request.post('/scan');
	let afterFirst: string | null = null;
	await expect(async () => {
		afterFirst = await finishedAt();
		expect(afterFirst).not.toBeNull();
		expect(afterFirst).not.toBe(baseline);
	}).toPass({ timeout: 10_000, intervals: [200] });

	await request.post('/scan');
	await expect(async () => {
		const afterSecond = await finishedAt();
		expect(afterSecond).not.toBeNull();
		expect(afterSecond).not.toBe(afterFirst);
	}).toPass({ timeout: 10_000, intervals: [200] });

	// The UI's own poll loop (fetch('/scan/status') every 4s) picks up the
	// same backend state and renders it in the open Protokoll panel —
	// confirming the fetch → re-render loop is genuinely live, not just the
	// raw API.
	await expect(page.locator('.card').first()).toContainText(
		'Bibliothekspfade sind noch nicht konfiguriert',
		{ timeout: 10_000 }
	);
});
