import { expect, test } from '@playwright/test';
import { itemId } from './_fixtures';

// Hearts and "Lesezeichen setzen" are client-side (static/js/app.js, static/js/player/main.js);
// the HTML around them is covered by tests/integration/web_collections.rs. Fixtures from
// e2e/seed.sql, in a fresh database per run.

test('the album heart toggles and the album shows up under Favoriten', async ({ page }) => {
	const albumId = await itemId(page.request, 'E2E-Fixture-Album-Favorite');
	await page.goto(`/library/albums/${albumId}`);
	const heart = page.getByRole('button', { name: 'Zu Favoriten hinzufügen' });
	await heart.click();
	const pressed = page.getByRole('button', { name: 'Aus Favoriten entfernen' });
	await expect(pressed).toHaveAttribute('aria-pressed', 'true');

	await page.getByRole('link', { name: 'Favoriten', exact: true }).click();
	await expect(page).toHaveURL(/\/library\/favorites$/);
	await expect(page.getByText('E2E-Fixture-Album-Favorite')).toBeVisible();

	// A reload shows the stored state, and taking it back empties the list again.
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Aus Favoriten entfernen' }).click();
	await expect(page.getByRole('button', { name: 'Zu Favoriten hinzufügen' })).toBeVisible();
	await page.goto('/library/favorites');
	await expect(page.getByText('E2E-Fixture-Album-Favorite')).toHaveCount(0);
});

test('a track heart marks the track as a favourite song', async ({ page }) => {
	const albumId = await itemId(page.request, 'E2E-Fixture-Album-Favorite');
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Titel favorisieren' }).first().click();
	await expect(
		page.getByRole('button', { name: 'Titel aus Favoriten entfernen' }).first()
	).toHaveAttribute('aria-pressed', 'true');
	await page.goto('/library/favorites');
	await expect(page.getByRole('table', { name: 'Songs' })).toContainText('Track One');
});

test('"Lesezeichen setzen" bookmarks the saved position and lists it', async ({ page }) => {
	const bookId = await itemId(page.request, 'E2E-Fixture-Book-Bookmark');
	await page.goto(`/library/books/${bookId}`);
	await page.getByRole('button', { name: 'Lesezeichen setzen' }).click();
	await expect(page.getByRole('button', { name: 'Lesezeichen · 1' })).toBeVisible();
	await page.getByRole('button', { name: 'Lesezeichen · 1' }).click();
	await expect(
		page.getByRole('button', { name: 'Lesezeichen 0:12:34 abspielen' })
	).toBeVisible();

	await page.getByRole('link', { name: 'Lesezeichen', exact: true }).click();
	await expect(page).toHaveURL(/\/library\/bookmarks$/);
	await expect(page.getByRole('table', { name: 'Lesezeichen' })).toContainText(
		'E2E-Fixture-Book-Bookmark'
	);
	await expect(page.getByText('0:12:34').first()).toBeVisible();
});
