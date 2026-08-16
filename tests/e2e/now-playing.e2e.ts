import { expect, test, type Page } from '@playwright/test';
import { eq } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';

async function ensureNowPlayingBook(): Promise<number> {
	const title = 'E2E-Fixture-NowPlaying-Book';
	const [existing] = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(eq(itemsTable.title, title));
	if (existing) return existing.id;
	const [book] = await db
		.insert(itemsTable)
		.values({
			kind: 'book',
			title,
			sortTitle: title.toLowerCase(),
			author: 'E2E Fixture Author'
		})
		.returning();
	await db
		.insert(tracksTable)
		.values([
			{ itemId: book.id, position: 1, path: `/fixtures/${book.id}/01.mp3`, duration: 3600 }
		]);
	return book.id;
}

/**
 * Navigates client-side, the way an in-app `<a href>` click would, instead of
 * `page.goto()`. There's no in-app link to the fullscreen player yet (it's
 * only reachable by URL), but `page.goto()` is a real top-level browser
 * navigation — it tears down the page's JS and, with it, the in-memory
 * player store (`src/lib/player.svelte.ts` holds `current` purely in
 * memory, nothing persists it across a reload). Confirmed empirically that
 * this breaks the round trip two ways: (1) `page.goto()` to `/player` wipes
 * whatever the previous page started playing, and (2) the close button's
 * `history.back()` *also* hard-reloads back to a `page.goto()`-loaded entry,
 * because Playwright's CDP session disables the back/forward cache a real
 * browser would normally use to restore that entry without a reload — so
 * the mini-player bar's state is wiped a second time on the way back,
 * regardless of which item id is involved. Dispatching a trusted click on an
 * injected same-origin `<a>` lets SvelteKit's own client router handle the
 * navigation (a soft, pushState-based transition), which is what keeps the
 * player store alive across the round trip and lets `history.back()`
 * restore the previous route without reloading — exactly the mechanism a
 * real "open fullscreen player" link would use once one exists.
 */
async function clientSideGoto(page: Page, url: string) {
	await page.evaluate((href) => {
		const a = document.createElement('a');
		a.href = href;
		document.body.appendChild(a);
		a.click();
		a.remove();
	}, url);
	await page.waitForURL(`**${url}`);
}

test('the fullscreen player has no sidebar and no mini-player bar', async ({ page }) => {
	await page.goto('/library/1/player');
	await expect(page.locator('nav.sidebar')).not.toBeVisible();
	await expect(page.locator('.bar')).not.toBeVisible();
});

test('closing returns to the previous library page with the mini-player bar restored', async ({
	page
}) => {
	const bookId = await ensureNowPlayingBook();
	await page.goto(`/library/books/${bookId}`);
	await page.getByRole('button', { name: 'Abspielen' }).click();
	await expect(page.locator('.bar')).toBeVisible();

	await clientSideGoto(page, `/library/${bookId}/player`);
	await page.getByRole('button', { name: 'Schließen' }).click();

	await expect(page).toHaveURL(`/library/books/${bookId}`);
	await expect(page.locator('.bar')).toBeVisible();
});
