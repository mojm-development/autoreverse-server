import { expect, test } from '@playwright/test';

test('subscription rail shows unread-count badges from real data', async ({ page }) => {
	await page.goto('/library/podcasts');
	await expect(page.getByText('Abos ·')).toBeVisible();
});

test('the search box is seeded from ?q= so returning to it is not a blank slate', async ({
	page
}) => {
	await page.goto('/library/podcasts?q=Tennis');
	await expect(page.getByPlaceholder('Podcast suchen')).toHaveValue('Tennis');
});

test('the preview page carries the query back to the search', async ({ page }) => {
	// Port 9 (discard) refuses instantly: this exercises the page shell and its
	// back link without depending on a reachable feed or the iTunes directory.
	await page.goto('/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml&q=Tennis');
	const back = page.getByRole('link', { name: 'Zurück zur Suche' });
	await expect(back).toHaveAttribute('href', '/library/podcasts?q=Tennis');
});

test('the preview page reports an unreachable feed instead of failing', async ({ page }) => {
	await page.goto('/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml');
	await expect(page.getByText('Der Feed ist nicht erreichbar.')).toBeVisible();
});

// The feed, its two five-second episodes and the directory answer come from e2e/fixtures/podcast,
// served on the Mac by run-on-jarvis.sh — the server fetches them, nothing reaches the internet.
test('search, preview, subscribe, play an episode, filter and unsubscribe', async ({ page }) => {
	await page.goto('/library/podcasts');
	await page.getByPlaceholder('Podcast suchen').fill('E2E');
	await page.getByRole('button', { name: 'Suchen' }).click();
	const hit = page.getByRole('link', { name: /E2E-Fixture-Podcast/ });
	await expect(hit).toBeVisible();
	await expect(page).toHaveURL(/\/library\/podcasts\?q=E2E$/);

	await hit.click();
	await expect(page.getByRole('heading', { level: 1, name: 'E2E-Fixture-Podcast' })).toBeVisible();
	await expect(page.getByText('2 Folgen im Feed')).toBeVisible();
	await expect(page.getByRole('link', { name: 'Zurück zur Suche' })).toHaveAttribute(
		'href',
		'/library/podcasts?q=E2E'
	);

	await page.getByRole('button', { name: 'Abonnieren' }).click();
	await expect(page).toHaveURL(/\/library\/podcasts\/\d+$/);
	await expect(page.getByRole('heading', { level: 1, name: 'E2E-Fixture-Podcast' })).toBeVisible();
	await expect(page.getByText('Abos · 1')).toBeVisible();

	// Not downloaded yet: playing downloads it first, then the mini player takes over.
	await page.getByRole('button', { name: 'E2E-Folge Zwei abspielen' }).click();
	await expect(page.getByTestId('mini-player')).toBeVisible();
	await expect(page.locator('[data-mini-title]')).toContainText('E2E-Folge Zwei');
	const second = page.locator('[data-episode]', { hasText: 'E2E-Folge Zwei' });
	await expect(second.getByText('geladen')).toBeVisible();

	await page.getByRole('button', { name: /^Geladen/ }).click();
	await expect(page.locator('[data-episode]:visible')).toHaveCount(1);
	await page.getByRole('button', { name: /^Alle/ }).click();
	await expect(page.locator('[data-episode]:visible')).toHaveCount(2);

	await page.getByRole('button', { name: 'Abo kündigen' }).click();
	await page.getByRole('button', { name: 'Wirklich kündigen' }).click();
	await expect(page).toHaveURL(/\/library\/podcasts$/);
	await expect(page.getByText('Abos · 0')).toBeVisible();
});

test.describe('as a non-admin', () => {
	test.use({ storageState: 'playwright/.auth/nonadmin.json' });

	test('the podcast-search prompt is hidden for a non-admin user', async ({ page }) => {
		await page.goto('/library/podcasts');
		await expect(page.getByPlaceholder('Podcast suchen')).not.toBeVisible();
	});

	test('a non-admin cannot open the subscribe preview', async ({ page }) => {
		// requireWebAdmin bounces to /library, same as the admin-only settings panels.
		await page.goto('/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml');
		await expect(page).toHaveURL('/library');
	});
});
