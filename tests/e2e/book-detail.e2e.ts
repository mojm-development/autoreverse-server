import { expect, test } from '@playwright/test';
import { eq, and } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import {
	items as itemsTable,
	tracks as tracksTable,
	progress as progressTable,
	chapters as chaptersTable,
	users
} from '../../src/lib/server/db/schema';

async function ensureBook(
	title: string,
	narrator: string | null,
	withProgress: boolean,
	progressPosition?: number
): Promise<number> {
	const [existing] = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(eq(itemsTable.title, title));
	let bookId: number;
	if (existing) {
		bookId = existing.id;
	} else {
		const [book] = await db
			.insert(itemsTable)
			.values({
				kind: 'book',
				title,
				sortTitle: title.toLowerCase(),
				author: 'E2E Fixture Author',
				narrator
			})
			.returning();
		bookId = book.id;
		await db
			.insert(tracksTable)
			.values([
				{ itemId: bookId, position: 1, path: `/fixtures/${bookId}/01.mp3`, duration: 3600 }
			]);
	}
	if (withProgress) {
		const [userRow] = await db.select({ id: users.id }).from(users).where(eq(users.name, 'Oliver'));
		// Fall back gracefully if the auth-setup fixture user hasn't run yet in this worker.
		if (userRow) {
			const [existingProgress] = await db
				.select()
				.from(progressTable)
				.where(and(eq(progressTable.userId, userRow.id), eq(progressTable.itemId, bookId)));
			if (!existingProgress) {
				await db.insert(progressTable).values({
					userId: userRow.id,
					itemId: bookId,
					position: progressPosition ?? 1234,
					finished: false
				});
			}
		}
	}
	return bookId;
}

test('shows "Weiter ab" with the formatted resume position when progress exists', async ({
	page
}) => {
	const bookId = await ensureBook('E2E-Fixture-Book-WithProgress', 'A Narrator', true);
	await page.goto(`/library/books/${bookId}`);
	await expect(page.getByRole('button', { name: /Weiter ab \d+:\d{2}:\d{2}/ })).toBeVisible();
});

test('the narrator line is omitted entirely when narrator is null, not shown as "gelesen von null"', async ({
	page
}) => {
	const bookId = await ensureBook('E2E-Fixture-Book-NoNarrator', null, false);
	await page.goto(`/library/books/${bookId}`);
	await expect(page.getByText('gelesen von null')).not.toBeVisible();
});

test('"Von vorn" button restarts playback from position 0', async ({ page }) => {
	// Create a book with progress at 500 seconds (well into the track)
	const bookId = await ensureBook('E2E-Fixture-Book-Restart', 'A Narrator', true, 500);
	await page.goto(`/library/books/${bookId}`);

	// Click the "Von vorn" button
	await page.getByRole('button', { name: 'Von vorn' }).click();

	// Wait for the MiniPlayerBar to appear and show time at 0:00
	await expect(page.locator('.time').first()).toContainText('0:00');
});

test('hero chapter label shows the last chapter when progress is at/past the end', async ({
	page
}) => {
	// Create a book with multiple chapters
	const [book] = await db
		.insert(itemsTable)
		.values({
			kind: 'book',
			title: 'E2E-Fixture-Book-EndOfBook',
			sortTitle: 'e2e-fixture-book-endofbook',
			author: 'E2E Fixture Author',
			narrator: 'A Narrator'
		})
		.returning();

	const bookId = book.id;

	// Insert a single track covering all chapters
	await db
		.insert(tracksTable)
		.values([{ itemId: bookId, position: 1, path: `/fixtures/${bookId}/01.mp3`, duration: 3600 }]);

	// Insert chapters: chapter 1 (0-1200), chapter 2 (1200-2400), chapter 3 (2400-3600)
	const chapters = [
		{ itemId: bookId, position: 1, title: 'Chapter 1: Beginning', start: 0, end: 1200 },
		{ itemId: bookId, position: 2, title: 'Chapter 2: Middle', start: 1200, end: 2400 },
		{ itemId: bookId, position: 3, title: 'Chapter 3: End', start: 2400, end: 3600 }
	];
	await db.insert(chaptersTable).values(chapters);

	// Create progress positioned at/past the last chapter's end
	const [userRow] = await db.select({ id: users.id }).from(users).where(eq(users.name, 'Oliver'));
	if (userRow) {
		await db
			.insert(progressTable)
			.values({
				userId: userRow.id,
				itemId: bookId,
				position: 3600, // At the very end
				finished: false
			})
			.onConflictDoUpdate({
				target: [progressTable.userId, progressTable.itemId],
				set: { position: 3600 }
			});
	}

	// Navigate to the book detail page
	await page.goto(`/library/books/${bookId}`);

	// The hero's chapter-name label should show "Chapter 3: End", not "Chapter 1: Beginning"
	const chapterNameLocator = page.locator('.progress-row .chapter-name');
	await expect(chapterNameLocator).toContainText('Chapter 3: End');
});
