import { expect, test } from '@playwright/test';
import { db } from '../../src/lib/server/db';
import { items, tracks } from '../../src/lib/server/db/schema';
import { eq } from 'drizzle-orm';

test('searching updates the URL query and shows grouped result counts', async ({ page }) => {
	await page.goto('/library/search');
	await page.getByRole('searchbox').fill('eschbach');
	await page.getByRole('searchbox').press('Enter');
	await expect(page).toHaveURL(/q=eschbach/);
	await expect(page.getByText(/Treffer in \d+ Items/)).toBeVisible();
});

test('track-only search results are visible when no albums match', async ({ page }) => {
	// Seed a test item and track with distinctive titles
	const testAlbumTitle = 'Zzz-Test-Album-9182-NoMatch';
	const testTrackTitle = 'Zzz-Test-Track-Only-9182';

	// Check if test data already exists (idempotent)
	const [existingItem] = await db
		.select({ id: items.id })
		.from(items)
		.where(eq(items.title, testAlbumTitle));

	if (!existingItem) {
		// Insert test album
		const insertResult = await db
			.insert(items)
			.values({
				kind: 'album',
				title: testAlbumTitle,
				sortTitle: testAlbumTitle,
				artist: 'Test Artist'
			})
			.returning({ id: items.id });
		const itemId = insertResult[0].id;

		// Insert test track
		await db.insert(tracks).values({
			itemId: itemId,
			position: 1,
			path: `/test/paths/${testTrackTitle}.mp3`,
			duration: 300,
			title: testTrackTitle
		});
	}

	// Search for the track by its distinctive title
	await page.goto('/library/search');
	await page.getByRole('searchbox').fill(testTrackTitle);
	await page.getByRole('searchbox').press('Enter');

	// Assert the track title is visible in the results
	await expect(page.getByText(testTrackTitle)).toBeVisible();
});
