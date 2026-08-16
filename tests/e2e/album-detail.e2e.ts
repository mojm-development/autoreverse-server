import { expect, test } from '@playwright/test';
import { eq } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';

const FIXTURE_TITLE = 'E2E-Fixture-Album-Detail';

async function ensureFixtureAlbum(): Promise<number> {
	const [existing] = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(eq(itemsTable.title, FIXTURE_TITLE));
	if (existing) return existing.id;
	const [album] = await db
		.insert(itemsTable)
		.values({
			kind: 'album',
			title: FIXTURE_TITLE,
			sortTitle: FIXTURE_TITLE.toLowerCase(),
			artist: 'E2E Fixture Artist'
		})
		.returning();
	await db.insert(tracksTable).values([
		{
			itemId: album.id,
			position: 1,
			path: `/fixtures/${album.id}/01.mp3`,
			duration: 180,
			title: 'Track One'
		},
		{
			itemId: album.id,
			position: 2,
			path: `/fixtures/${album.id}/02.mp3`,
			duration: 210,
			title: 'Track Two'
		}
	]);
	return album.id;
}

test('clicking Abspielen starts playback and the mini-player bar appears', async ({ page }) => {
	const albumId = await ensureFixtureAlbum();
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Abspielen' }).click();
	await expect(page.locator('.bar')).toBeVisible();
});

test('the currently-playing track row is visually distinguished', async ({ page }) => {
	const albumId = await ensureFixtureAlbum();
	await page.goto(`/library/albums/${albumId}`);
	await page.getByRole('button', { name: 'Abspielen' }).click();
	await expect(page.locator('[aria-current="true"][role="row"]')).toBeVisible();
});
