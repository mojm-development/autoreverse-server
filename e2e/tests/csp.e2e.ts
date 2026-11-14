import { expect, test } from '@playwright/test';
import { itemId } from './_fixtures';

// The server sends a Content-Security-Policy without inline scripts (src/app.rs). Anything the
// UI still does inline would be blocked silently — so every main page is opened and the browser's
// own violation reports are collected.

test.beforeEach(async ({ page }) => {
	await page.addInitScript(() => {
		const seen: string[] = [];
		(window as unknown as { __csp: string[] }).__csp = seen;
		document.addEventListener('securitypolicyviolation', (event) => {
			seen.push(`${event.violatedDirective} ${event.blockedURI} ${event.sourceFile}:${event.lineNumber}`);
		});
	});
});

async function violations(page: import('@playwright/test').Page): Promise<string[]> {
	return page.evaluate(() => (window as unknown as { __csp: string[] }).__csp);
}

test('no page trips the Content-Security-Policy', async ({ page, request }) => {
	test.setTimeout(90_000);
	const album = await itemId(request, 'E2E-Fixture-Album-ForArtist');
	const paths = [
		'/library',
		'/library/albums',
		`/library/albums/${album}`,
		'/library/artists',
		'/library/books',
		'/library/podcasts',
		'/library/playlists',
		'/library/favorites',
		'/library/search?q=e',
		`/library/${album}/player`,
		'/settings/libraries',
		'/settings/scan',
		'/settings/playback',
		'/settings/users',
		'/settings/security'
	];
	for (const path of paths) {
		const response = await page.goto(path);
		expect(response?.headers()['content-security-policy'], path).toContain("script-src 'self'");
		await page.waitForLoadState('networkidle');
		// htmx and the page scripts ran.
		expect(await page.evaluate(() => typeof (window as unknown as { htmx?: unknown }).htmx), path).toBe(
			'object'
		);
		expect(await violations(page), path).toEqual([]);
	}
});

test('the stored theme still applies before paint', async ({ page }) => {
	await page.goto('/library');
	await page.evaluate(() => localStorage.setItem('autoreverse-theme', 'light'));
	await page.reload();
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
	await page.evaluate(() => localStorage.removeItem('autoreverse-theme'));
	expect(await violations(page)).toEqual([]);
});
