import { expect, test } from '@playwright/test';
import { eq, inArray } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import {
	users,
	items as itemsTable,
	tracks as tracksTable,
	playlists as playlistsTable,
	playlistEntries
} from '../../src/lib/server/db/schema';

const PLAYLIST_NAME = 'E2E-Fixture-Playlist';
const TITLES_IN_ORDER = ['Erster Titel', 'Zweiter Titel', 'Dritter Titel'];

// Re-run safety: the e2e DB is a persistent shared dev Postgres, not reset
// between runs (see auth.setup.ts's comment on the same). The reordering
// test itself mutates this fixture's entry order as a side effect of
// passing, so a second run against an already-reordered playlist must not
// see stale positions — reset them back to the canonical Erster/Zweiter/
// Dritter order every time, not just create-if-missing.
async function resetEntryOrder(playlistId: number, trackIdByTitle: Map<string, number>) {
	const entries = await db
		.select({ id: playlistEntries.id, trackId: playlistEntries.trackId })
		.from(playlistEntries)
		.where(eq(playlistEntries.playlistId, playlistId));
	// Phase 1: park every entry at a negative sentinel position so phase 2's
	// writes can't collide with the (playlist_id, position) unique index.
	for (const [i, entry] of entries.entries()) {
		await db
			.update(playlistEntries)
			.set({ position: -(i + 1) })
			.where(eq(playlistEntries.id, entry.id));
	}
	// Phase 2: assign the canonical position for each entry's track title.
	const idToTitle = new Map([...trackIdByTitle].map(([title, id]) => [id, title]));
	for (const entry of entries) {
		const title = idToTitle.get(entry.trackId!);
		const position = TITLES_IN_ORDER.indexOf(title!) + 1;
		await db.update(playlistEntries).set({ position }).where(eq(playlistEntries.id, entry.id));
	}
}

async function ensurePlaylist(): Promise<number> {
	const [user] = await db.select({ id: users.id }).from(users).where(eq(users.name, 'Oliver'));
	const [existing] = await db
		.select({ id: playlistsTable.id })
		.from(playlistsTable)
		.where(eq(playlistsTable.name, PLAYLIST_NAME));
	if (existing) {
		const existingEntries = await db
			.select({ trackId: playlistEntries.trackId })
			.from(playlistEntries)
			.where(eq(playlistEntries.playlistId, existing.id));
		const trackRows = await db
			.select({ id: tracksTable.id, title: tracksTable.title })
			.from(tracksTable)
			.where(
				inArray(
					tracksTable.id,
					existingEntries.map((e) => e.trackId!)
				)
			);
		const trackIdByTitle = new Map(trackRows.map((t) => [t.title!, t.id]));
		await resetEntryOrder(existing.id, trackIdByTitle);
		return existing.id;
	}

	const [playlist] = await db
		.insert(playlistsTable)
		.values({ userId: user.id, name: PLAYLIST_NAME })
		.returning();
	for (const [i, title] of TITLES_IN_ORDER.entries()) {
		const [track] = await db
			.insert(itemsTable)
			.values({ kind: 'album', title, sortTitle: title.toLowerCase() })
			.returning()
			.then(async ([album]) => {
				const [t] = await db
					.insert(tracksTable)
					.values({
						itemId: album.id,
						position: 1,
						path: `/fixtures/playlist/${album.id}.mp3`,
						duration: 200,
						title
					})
					.returning();
				return [t];
			});
		await db
			.insert(playlistEntries)
			.values({ playlistId: playlist.id, trackId: track.id, position: i + 1 });
	}
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
	const playlistId = await ensurePlaylist();
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
