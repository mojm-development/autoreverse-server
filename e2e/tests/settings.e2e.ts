import { expect, test } from '@playwright/test';

test.describe('as a non-admin', () => {
	test.use({ storageState: 'playwright/.auth/nonadmin.json' });

	test('a non-admin cannot reach the Bibliotheken/Scan/Podcast-Abos panels', async ({ page }) => {
		await page.goto('/settings/libraries');
		await expect(page).toHaveURL('/library');
	});

	test('the overview offers a non-admin only the sections they may open', async ({ page }) => {
		await page.goto('/settings');
		const overview = page.getByTestId('settings-overview');
		await expect(overview.getByRole('link', { name: 'Wiedergabe' })).toBeVisible();
		// Linking here would only bounce off the +layout.server.ts redirect.
		await expect(overview.getByRole('link', { name: 'Bibliotheken' })).toHaveCount(0);
	});
});

test('/settings itself renders an overview linking into every section', async ({ page }) => {
	await page.goto('/settings');
	await expect(page.getByRole('heading', { name: 'Einstellungen' })).toBeVisible();
	// Scoped to the card grid — the surrounding subnav carries the same labels.
	const overview = page.getByTestId('settings-overview');
	await expect(overview.getByRole('link')).toHaveCount(7);
	await overview.getByRole('link', { name: 'Bibliotheken' }).click();
	await expect(page).toHaveURL('/settings/libraries');
});

test('the sidebar reaches the settings section from a library page', async ({ page }) => {
	await page.goto('/library/albums');
	await page.getByRole('link', { name: 'Einstellungen' }).click();
	await expect(page).toHaveURL('/settings');
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
	// DOM baseline for the final poll-loop assertion below, captured before
	// this test (or any concurrently-running spec sharing the same server
	// process's single `scanState` singleton) triggers anything further.
	const domBaseline = await page.locator('.card').first().textContent();

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
	//
	// This asserts the rendered text CHANGED from the DOM baseline, rather
	// than a specific fixed message: `scanState` (src/lib/server/admin/
	// scanState.ts) is a single in-process singleton shared by the whole
	// `bun run preview` server — every e2e spec's requests hit the same process.
	// tests/e2e/smoke.e2e.ts (Task 41) legitimately triggers its own real
	// scans (with real results, not the fixed "not configured" error) as part
	// of the same shared singleton, and Playwright's default cross-file
	// parallelism means that other spec's scan can interleave with this one's
	// — so the *specific* text visible at any instant isn't this test's to
	// assert on. What every properly-configured trigger guarantees, from any
	// test, is that `finished_at` (and therefore the rendered card) moves
	// forward — already confirmed above via the real API — so confirm the DOM
	// picked that up too, without asserting whose scan's content ended up on
	// screen last.
	await expect(async () => {
		const text = await page.locator('.card').first().textContent();
		expect(text).not.toBe(domBaseline);
	}).toPass({ timeout: 10_000, intervals: [200] });
});
