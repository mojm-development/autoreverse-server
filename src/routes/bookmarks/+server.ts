import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { eq, sql } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item } from '$lib/server/library/queries';
import { addBookmark, bookmarksForItem } from '$lib/server/library/bookmarks';
import {
	bookmarks,
	items as itemsTable,
	type bookmarks as BookmarksType
} from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

type Bookmark = typeof BookmarksType.$inferSelect;

export async function _bookmarksGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const itemIdParam = event.url.searchParams.get('item_id');
		const rows = itemIdParam
			? await bookmarksForItem(db, userId, Number(itemIdParam))
			: await db
					.select({ bookmark: bookmarks })
					.from(bookmarks)
					.innerJoin(itemsTable, eq(itemsTable.id, bookmarks.itemId))
					.where(eq(bookmarks.userId, userId))
					.orderBy(sql`lower(${itemsTable.sortTitle})`, bookmarks.position)
					.then((rows) => rows.map((r) => r.bookmark));
		return json({
			bookmarks: rows.map((r: Bookmark) => ({
				id: r.id,
				item_id: r.itemId,
				position: r.position,
				title: r.title
			}))
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _bookmarksPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { item_id, position, title } = await event.request.json();
		if (typeof item_id !== 'number' || !Number.isFinite(item_id))
			return apiError(422, 'item_id muss eine Zahl sein');
		if (typeof position !== 'number' || position < 0)
			return apiError(422, 'position muss ≥ 0 sein');
		if (typeof title !== 'string' || title.length < 1 || title.length > 200)
			return apiError(422, 'title muss 1–200 Zeichen haben');
		const row = await item(db, item_id);
		if (!row) return apiError(404, 'Unbekanntes Item');
		const bookmark = await addBookmark(db, userId, item_id, position, title);
		return json(
			{
				id: bookmark.id,
				item_id: bookmark.item_id,
				position: bookmark.position,
				title: bookmark.title
			},
			{ status: 201 }
		);
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => _bookmarksGetHandler(defaultDb, event);
export const POST: RequestHandler = (event) => _bookmarksPostHandler(defaultDb, event);
