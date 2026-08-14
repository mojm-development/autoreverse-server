import { and, eq, sql } from 'drizzle-orm';
import { bookmarks } from '../db/schema';
import type { DrizzleDb } from '../db';

export async function addBookmark(
	db: DrizzleDb,
	userId: number,
	itemId: number,
	position: number,
	title: string
) {
	const rows = await db.execute(sql`
		INSERT INTO bookmarks (user_id, item_id, position, title)
		VALUES (${userId}, ${itemId}, ${position}, ${title})
		ON CONFLICT (user_id, item_id, position) DO UPDATE SET title = EXCLUDED.title
		RETURNING id, item_id, position, title
	`);
	return rows[0] as { id: number; item_id: number; position: number; title: string };
}
export async function bookmarksForItem(db: DrizzleDb, userId: number, itemId: number) {
	return db
		.select()
		.from(bookmarks)
		.where(and(eq(bookmarks.userId, userId), eq(bookmarks.itemId, itemId)))
		.orderBy(bookmarks.position);
}
export async function removeBookmark(
	db: DrizzleDb,
	userId: number,
	bookmarkId: number
): Promise<boolean> {
	const rows = await db
		.delete(bookmarks)
		.where(and(eq(bookmarks.id, bookmarkId), eq(bookmarks.userId, userId)))
		.returning({ id: bookmarks.id });
	return rows.length > 0;
}

export async function countBookmarks(db: DrizzleDb, userId: number): Promise<number> {
	const rows = await db.execute(
		sql`SELECT count(*)::int AS n FROM bookmarks WHERE user_id = ${userId}`
	);
	return (rows[0] as { n: number }).n;
}
