import { expect, test } from '@playwright/test';
import { eq, and } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import {
	items as itemsTable,
	tracks as tracksTable,
	progress as progressTable,
	users
} from '../../src/lib/server/db/schema';

async function ensureBook(
	title: string,
	narrator: string | null,
	withProgress: boolean
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
				await db
					.insert(progressTable)
					.values({ userId: userRow.id, itemId: bookId, position: 1234, finished: false });
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
