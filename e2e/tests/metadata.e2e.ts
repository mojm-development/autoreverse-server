import { expect, test, type Page } from '@playwright/test';
import { itemId } from './_fixtures';

// Metadata editing as a Verwalter ('Oliver'): the editor on the book and album pages, the bulk
// editor with preview, apply and undo, the series assistant, and the artist picture. Fixtures in
// e2e/seed.sql; results are checked over the API as well as on the page.

async function item(page: Page, id: number) {
	const res = await page.request.get(`/items/${id}`);
	expect(res.ok()).toBe(true);
	return res.json();
}

test('the book editor sets a field, locks it, and hands it back', async ({ page }) => {
	const id = await itemId(page.request, 'E2E-Fixture-Book-Edit');
	await page.goto(`/library/books/${id}`);
	await page.getByRole('button', { name: 'Bearbeiten' }).click();

	const editor = page.getByRole('dialog', { name: 'Metadaten bearbeiten' });
	await expect(editor).toBeVisible();
	const save = editor.getByRole('button', { name: 'Speichern' });
	await expect(save).toBeDisabled();
	await editor.locator('[data-field="narrator"] input').fill('Anna Sprecherin');
	await editor.locator('[data-field="year"] input').fill('2011');
	await expect(save).toBeEnabled();
	await save.click();

	await expect(editor).toBeHidden();
	await expect(page.locator('.subline')).toContainText('gelesen von Anna Sprecherin');
	expect((await item(page, id)).year).toBe(2011);

	// Reopened, the field says it is set by hand; handing it back sends a reset.
	await page.getByRole('button', { name: 'Bearbeiten' }).click();
	const lock = editor.locator('[data-field="narrator"] [data-lock]');
	await expect(lock).toHaveText('von Hand');
	await lock.click();
	await expect(lock).toHaveText('zurückgeben');
	const patch = page.waitForResponse((r) => r.url().endsWith(`/items/${id}`) && r.request().method() === 'PATCH');
	await editor.getByRole('button', { name: 'Speichern' }).click();
	const body = await (await patch).json();
	expect(body.locked_fields).not.toContain('narrator');
	expect(body.locked_fields).toContain('year');
});

test('the album editor renames a track and shows a refusal in the sheet', async ({ page }) => {
	const id = await itemId(page.request, 'E2E-Fixture-Album-Edit');
	await page.goto(`/library/albums/${id}`);
	await page.getByRole('button', { name: 'Bearbeiten' }).click();
	const editor = page.getByRole('dialog', { name: 'Metadaten bearbeiten' });
	await expect(editor.getByRole('heading', { name: 'Titel' })).toBeVisible();

	// A title cannot be emptied: the server says why, the sheet stays open.
	await editor.locator('[data-field="title"] input').fill('   ');
	await editor.getByRole('button', { name: 'Speichern' }).click();
	await expect(editor.locator('.error')).toHaveText('title darf nicht leer sein');
	await editor.locator('[data-field="title"] input').fill('E2E-Fixture-Album-Edit');

	await editor.locator('input[data-track-id]').fill('Shoot to Thrill');
	await editor.getByRole('button', { name: 'Speichern' }).click();
	await expect(editor).toBeHidden();
	await expect(page.getByRole('row', { name: /Shoot to Thrill/ })).toBeVisible();

	// Escape and "Abbrechen" close without saving.
	await page.getByRole('button', { name: 'Bearbeiten' }).click();
	await page.keyboard.press('Escape');
	await expect(editor).toBeHidden();
});

test('the bulk editor previews, applies and undoes a change to picked albums', async ({ page }) => {
	const a = await itemId(page.request, 'E2E-Bulk-Album-A');
	const b = await itemId(page.request, 'E2E-Bulk-Album-B');
	await page.goto('/library/albums?artist=E2E%20Bulk%20Artist');

	const bar = page.getByRole('status').filter({ hasText: 'ausgewählt' });
	await expect(bar).toBeHidden();
	await page.getByRole('button', { name: 'E2E-Bulk-Album-A auswählen' }).click();
	await page.getByRole('button', { name: 'E2E-Bulk-Album-B auswählen' }).click();
	await expect(page).toHaveURL(/artist=E2E/);
	await expect(bar).toContainText('2 ausgewählt');
	await bar.getByRole('button', { name: 'Bearbeiten' }).click();

	const sheet = page.getByRole('dialog', { name: 'Mehrere bearbeiten' });
	await expect(sheet.getByRole('heading', { name: '2 Einträge bearbeiten' })).toBeVisible();
	await expect(sheet.getByRole('button', { name: 'Anwenden' })).toBeDisabled();
	await sheet.locator('[data-field="year"] [data-field-toggle]').check();
	await sheet.locator('[data-field="year"] [data-field-value]').fill('1999');
	await sheet.getByRole('button', { name: 'Vorschau' }).click();
	await expect(sheet.locator('.preview-count')).toHaveText('2 von 2 werden geändert.');
	await expect(sheet.locator('.diff .row')).toHaveCount(2);
	expect((await item(page, a)).year).toBeNull();

	await sheet.getByRole('button', { name: 'Anwenden' }).click();
	await expect(sheet.locator('.done-title')).toHaveText('2 Einträge geändert');
	expect((await item(page, a)).year).toBe(1999);
	expect((await item(page, b)).year).toBe(1999);
	// The page underneath was reloaded: the selection is gone.
	await expect(bar).toBeHidden();

	await sheet.getByRole('button', { name: 'Rückgängig' }).click();
	await expect(sheet.locator('.undone')).toHaveText('Zurückgenommen: 2 Felder');
	expect((await item(page, a)).year).toBeNull();
	await sheet.getByRole('button', { name: 'Abbrechen' }).click();
	await expect(sheet).toBeHidden();
});

test('the last bulk change can still be undone after the sheet is closed', async ({ page }) => {
	const a = await itemId(page.request, 'E2E-Bulk-Album-A');
	await page.goto('/library/albums?artist=E2E%20Bulk%20Artist');
	await page.getByRole('button', { name: 'E2E-Bulk-Album-A auswählen' }).click();
	await page.getByRole('button', { name: 'E2E-Bulk-Album-B auswählen' }).click();
	await page.getByRole('status').filter({ hasText: 'ausgewählt' }).getByRole('button', { name: 'Bearbeiten' }).click();
	const sheet = page.getByRole('dialog', { name: 'Mehrere bearbeiten' });
	await sheet.locator('[data-field="year"] [data-field-toggle]').check();
	await sheet.locator('[data-field="year"] [data-field-value]').fill('2001');
	await sheet.getByRole('button', { name: 'Vorschau' }).click();
	await sheet.getByRole('button', { name: 'Anwenden' }).click();
	await expect(sheet.locator('.done-title')).toHaveText('2 Einträge geändert');
	await sheet.getByRole('button', { name: 'Fertig' }).click();
	await expect(sheet).toBeHidden();

	const line = page.locator('[data-last-bulk]');
	await expect(line).toContainText('Letzte Massenbearbeitung: 2 Felder an 2 Einträgen');
	await line.getByRole('button', { name: 'Rückgängig' }).click();
	await expect(line.getByRole('status')).toHaveText('Zurückgenommen: 2 Felder');
	expect((await item(page, a)).year).toBeNull();
	await expect(line.getByRole('button', { name: 'Rückgängig' })).toHaveCount(0);
});

test('the series assistant numbers picked books through', async ({ page }) => {
	const a = await itemId(page.request, 'E2E-Series-Book-A');
	const b = await itemId(page.request, 'E2E-Series-Book-B');
	await page.goto('/library/books?q=E2E-Series-Book');
	await page.getByRole('button', { name: 'E2E-Series-Book-A auswählen' }).click();
	// Shift-click picks the range.
	await page.getByRole('button', { name: 'E2E-Series-Book-B auswählen' }).click({ modifiers: ['Shift'] });
	await page.getByRole('status').filter({ hasText: '2 ausgewählt' }).getByRole('button', { name: 'Bearbeiten' }).click();

	const sheet = page.getByRole('dialog', { name: 'Mehrere bearbeiten' });
	await sheet.getByLabel('Serie zuweisen').check();
	await expect(sheet.getByRole('button', { name: 'Vorschau' })).toBeDisabled();
	await sheet.getByPlaceholder('Serienname').fill('E2E-Saga');
	await sheet.getByLabel('Erste Bandnummer').fill('3');
	await sheet.getByRole('button', { name: 'Vorschau' }).click();
	await expect(sheet.locator('.diff .row')).toHaveCount(2);
	await sheet.getByRole('button', { name: 'Anwenden' }).click();
	await expect(sheet.locator('.done-title')).toHaveText('2 Einträge geändert');
	await sheet.getByRole('button', { name: 'Fertig' }).click();

	const [first, second] = [await item(page, a), await item(page, b)];
	expect([first.series, second.series]).toEqual(['E2E-Saga', 'E2E-Saga']);
	await page.goto(`/library/books/${a}`);
	await expect(page.getByText('Serie · 2 Bände').first()).toBeVisible();
});

test('the artist page picks an album, uploads a picture and resets', async ({ page }) => {
	const artistUrl = '/library/artists/E2E%20Cover%20Artist';
	await page.goto(artistUrl);
	await page.getByRole('button', { name: /E2E-Cover-Album-Old/ }).click();
	await expect(page).toHaveURL('/library/artists');
	const cover = await (await page.request.get('/artists/E2E%20Cover%20Artist/cover')).json();
	expect(cover.item_id).toBe(await itemId(page.request, 'E2E-Cover-Album-Old'));

	await page.goto(artistUrl);
	await expect(page.getByRole('button', { name: /E2E-Cover-Album-Old/ })).toHaveAttribute('aria-pressed', 'true');

	// A 1×1 PNG.
	const png = Buffer.from(
		'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
		'base64'
	);
	await page.getByLabel('Bilddatei').setInputFiles({ name: 'bild.png', mimeType: 'image/png', buffer: png });
	await page.getByRole('button', { name: 'Hochladen' }).click();
	await expect(page).toHaveURL('/library/artists');
	expect((await (await page.request.get('/artists/E2E%20Cover%20Artist/cover')).json()).has_image).toBe(true);

	await page.goto(artistUrl);
	await expect(page.locator('.preview img')).toBeVisible();
	await page.getByRole('button', { name: 'Auf Zufall zurücksetzen' }).click();
	await expect(page.locator('.preview img')).toHaveCount(0);
	expect((await (await page.request.get('/artists/E2E%20Cover%20Artist/cover')).json()).has_image).toBe(false);
});
