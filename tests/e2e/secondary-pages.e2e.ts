import { expect, test } from '@playwright/test';
import { eq } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import { items as itemsTable } from '../../src/lib/server/db/schema';

const ARTIST_NAME = 'Ansa Volt';
const ALBUM_TITLE = 'E2E-Fixture-Album-ForArtist';

async function ensureArtistFixture(): Promise<void> {
	const [existing] = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(eq(itemsTable.title, ALBUM_TITLE));
	if (existing) return;
	await db.insert(itemsTable).values({
		kind: 'album',
		title: ALBUM_TITLE,
		sortTitle: ALBUM_TITLE.toLowerCase(),
		artist: ARTIST_NAME
	});
}

test('every sidebar link resolves to a real page, not a 404', async ({ page }) => {
	for (const path of [
		'/library/favorites',
		'/library/artists',
		'/library/series',
		'/library/bookmarks'
	]) {
		const response = await page.goto(path);
		expect(response?.status()).toBeLessThan(400);
	}
});

test('clicking an artist on /library/artists filters the Alben page to that artist', async ({
	page
}) => {
	await ensureArtistFixture();
	await page.goto('/library/artists');
	await page.getByText(ARTIST_NAME).click();
	await expect(page).toHaveURL(/\/library\/albums\?artist=/);
});
