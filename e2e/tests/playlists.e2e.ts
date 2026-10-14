import { expect, test } from '@playwright/test';

// Scheibe 6 (Playlists): /library/playlists gibt es im Rust-Server noch nicht. Mit der Scheibe
// wandert die Fixture-Playlist (drei Alben „Erster/Zweiter/Dritter Titel“ mit je einem Track,
// Reihenfolge bei jedem Lauf zurückgesetzt) nach e2e/seed.sql; legacy legte sie per drizzle an.
test.fixme();

const PLAYLIST_NAME = 'E2E-Fixture-Playlist';

test('only the "Eigene" filter pill is rendered — no non-functional Automatisch/Geteilt pills', async ({
	page
}) => {
	await page.goto('/library/playlists');
	await expect(page.getByText('Eigene')).toBeVisible();
	await expect(page.getByText('Automatisch')).not.toBeVisible();
	await expect(page.getByText('Geteilt')).not.toBeVisible();
});

test('reordering a playlist entry persists the new order after reload', async ({ page }) => {
	const res = await page.request.get('/playlists');
	const { playlists } = (await res.json()) as { playlists: { id: number; name: string }[] };
	const playlistId = playlists.find((p) => p.name === PLAYLIST_NAME)!.id;
	await page.goto(`/library/playlists/${playlistId}`);
	const rows = page.locator('[role="row"]');
	await expect(rows).toHaveCount(3);
	await expect(rows.nth(0)).toContainText('Erster Titel');

	// Drag the first row onto the third row's position — native HTML5 DnD,
	// Playwright's dragTo() fires real dragstart/dragover/drop events.
	// The app's own ondrop handler is async (await fetch, then
	// location.reload()) and does its own reload once the PUT resolves, so
	// this waits for that navigation rather than also calling page.reload()
	// itself. Two things were confirmed empirically while getting this test
	// stable: (1) calling page.reload() immediately after dragTo() races the
	// in-flight PUT — dragTo() resolves as soon as the synthetic mouseup/drop
	// fires, before the app's fetch necessarily completes, and the early
	// reload cancels that in-flight request (the fixture's untouched order
	// survived, proving the move never happened); (2) even after waiting for
	// the PUT response, an *additional* explicit page.reload() collides with
	// the app's own location.reload() and throws net::ERR_ABORTED — the app
	// already reloads for us, so the test only needs to wait for that load.
	const reloaded = page.waitForEvent('load');
	await rows.nth(0).dragTo(rows.nth(2));
	await reloaded;

	const reordered = page.locator('[role="row"]');
	await expect(reordered).toHaveCount(3);
	await expect(reordered.nth(2)).toContainText('Erster Titel');
});
