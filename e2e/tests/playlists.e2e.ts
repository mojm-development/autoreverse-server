import { expect, test } from '@playwright/test';
import { itemId } from './_fixtures';

// Fixtures from e2e/seed.sql, in a fresh database per run: Oliver's "E2E-Fixture-Playlist" with
// three single tracks "Erster/Zweiter/Dritter Titel", and "E2E-Fixture-Album-Playlist" to add.
// Legacy created the playlist through drizzle and reset its order before each run.

const PLAYLIST_NAME = 'E2E-Fixture-Playlist';

async function playlistId(page: import('@playwright/test').Page, name: string): Promise<number> {
	const res = await page.request.get('/playlists');
	const { playlists } = (await res.json()) as { playlists: { id: number; name: string }[] };
	const playlist = playlists.find((p) => p.name === name);
	if (!playlist) throw new Error(`Playlist "${name}" fehlt`);
	return playlist.id;
}

test('only the "Eigene" filter pill is rendered — no non-functional Automatisch/Geteilt pills', async ({
	page
}) => {
	await page.goto('/library/playlists');
	await expect(page.getByText('Eigene')).toBeVisible();
	await expect(page.getByText('Automatisch')).not.toBeVisible();
	await expect(page.getByText('Geteilt')).not.toBeVisible();
});

test('reordering a playlist entry persists the new order after reload', async ({ page }) => {
	const id = await playlistId(page, PLAYLIST_NAME);
	await page.goto(`/library/playlists/${id}`);
	const rows = page.locator('[role="row"]');
	await expect(rows).toHaveCount(3);
	await expect(rows.nth(0)).toContainText('Erster Titel');

	// Drag the first row onto the third — native HTML5 DnD. Legacy reloaded the whole page after
	// the PUT; here only the list is swapped (a playing player keeps playing), so wait for the PUT
	// and the new order instead of a load event.
	const moved = page.waitForResponse(
		(res) => res.request().method() === 'PUT' && res.url().includes(`/playlists/${id}/entries/`)
	);
	await rows.nth(0).dragTo(rows.nth(2));
	expect((await moved).status()).toBe(204);
	await expect(rows.nth(2)).toContainText('Erster Titel');

	await page.reload();
	const reordered = page.locator('[role="row"]');
	await expect(reordered).toHaveCount(3);
	await expect(reordered.nth(0)).toContainText('Zweiter Titel');
	await expect(reordered.nth(2)).toContainText('Erster Titel');

	// The keyboard way back: "nach oben" twice puts it first again.
	await page.getByRole('button', { name: 'Erster Titel nach oben' }).click();
	await expect(page.locator('[role="row"]').nth(1)).toContainText('Erster Titel');
	await page.getByRole('button', { name: 'Erster Titel nach oben' }).click();
	await expect(page.locator('[role="row"]').nth(0)).toContainText('Erster Titel');
});

test('create a playlist, add an album and a track from the album page, play, remove, delete', async ({
	page
}) => {
	const name = `E2E-Neu-${Date.now()}`;
	await page.goto('/library/playlists');
	await page.getByRole('textbox', { name: 'Name der neuen Playlist' }).fill(name);
	await page.getByRole('button', { name: 'Anlegen' }).click();
	await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
	await expect(page.getByText('Noch keine Titel.')).toBeVisible();

	const albumId = await itemId(page.request, 'E2E-Fixture-Album-Playlist');
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Zur Playlist hinzufügen', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Zur Playlist hinzufügen' });
	await expect(dialog).toBeVisible();
	await dialog.getByRole('button', { name }).click();
	await expect(dialog.getByRole('status')).toHaveText(`Zu „${name}“ hinzugefügt.`);
	await dialog.getByRole('button', { name: 'Schließen' }).click();
	await expect(dialog).toBeHidden();

	await page.getByRole('button', { name: 'Track One zur Playlist hinzufügen' }).click();
	await dialog.getByRole('button', { name }).click();
	await expect(dialog.getByRole('status')).toHaveText(`Zu „${name}“ hinzugefügt.`);
	await dialog.getByRole('button', { name: 'Schließen' }).click();

	await page.getByRole('link', { name: 'Playlists', exact: true }).click();
	await expect(page).toHaveURL(/\/library\/playlists$/);
	const tile = page.getByRole('link', { name });
	await expect(tile).toContainText('2 Titel · 0.1 h');
	await tile.click();
	const rows = page.locator('[role="row"]');
	await expect(rows).toHaveCount(2);
	await expect(rows.nth(0)).toContainText('E2E-Fixture-Album-Playlist');
	await expect(rows.nth(1)).toContainText('Track One');

	// Playing goes through the mini player, which stays when the page changes.
	await page.getByRole('button', { name: 'Abspielen', exact: true }).click();
	await expect(page.getByTestId('mini-player')).toBeVisible();

	await page.getByRole('button', { name: 'Track One entfernen' }).click();
	await expect(page.locator('[role="row"]')).toHaveCount(1);
	await expect(page.getByTestId('mini-player')).toBeVisible();

	await page.getByText('Umbenennen').click();
	await page.getByRole('textbox', { name: 'Neuer Name' }).fill(`${name}-umbenannt`);
	await page.getByRole('button', { name: 'Speichern' }).click();
	await expect(page.getByRole('heading', { level: 1, name: `${name}-umbenannt` })).toBeVisible();

	page.once('dialog', (confirm) => void confirm.accept());
	await page.getByRole('button', { name: 'Löschen' }).click();
	await expect(page).toHaveURL(/\/library\/playlists$/);
	await expect(page.getByText(`${name}-umbenannt`)).toHaveCount(0);
});
