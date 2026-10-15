import { expect, test } from '@playwright/test';
import { itemId } from './_fixtures';

// The books, their progress for 'Oliver' and the end-of-book chapters are seeded by e2e/seed.sql
// (legacy inserted them through drizzle; the browser VM has no database access). The seed resets
// the progress positions on every run.

test('shows "Weiter ab" with the formatted resume position when progress exists', async ({
	page
}) => {
	const bookId = await itemId(page.request, 'E2E-Fixture-Book-WithProgress');
	await page.goto(`/library/books/${bookId}`);
	await expect(page.getByRole('button', { name: /Weiter ab \d+:\d{2}:\d{2}/ })).toBeVisible();
});

test('the narrator line is omitted entirely when narrator is null, not shown as "gelesen von null"', async ({
	page
}) => {
	const bookId = await itemId(page.request, 'E2E-Fixture-Book-NoNarrator');
	await page.goto(`/library/books/${bookId}`);
	await expect(page.getByText('gelesen von null')).not.toBeVisible();
});

test('"Von vorn" button restarts playback from position 0', async ({ page }) => {
	// Seeded with progress at 500 seconds (well into the track)
	const bookId = await itemId(page.request, 'E2E-Fixture-Book-Restart');
	await page.goto(`/library/books/${bookId}`);

	// Click the "Von vorn" button
	await page.getByRole('button', { name: 'Von vorn' }).click();

	// Wait for the MiniPlayerBar to appear and show time at 0:00
	await expect(page.locator('.time').first()).toContainText('0:00');
});

test('hero chapter label shows the last chapter when progress is at/past the end', async ({
	page
}) => {
	// Seeded: chapters 0-1200, 1200-2400, 2400-3600 and progress at 3600 (the very end)
	const bookId = await itemId(page.request, 'E2E-Fixture-Book-EndOfBook');

	// Navigate to the book detail page
	await page.goto(`/library/books/${bookId}`);

	// The hero's chapter-name label should show "Chapter 3: End", not "Chapter 1: Beginning"
	const chapterNameLocator = page.locator('.progress-row .chapter-name');
	await expect(chapterNameLocator).toContainText('Chapter 3: End');
});
