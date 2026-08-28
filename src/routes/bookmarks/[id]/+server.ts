import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { removeBookmark } from '$lib/server/library/bookmarks';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _bookmarkDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const deleted = await removeBookmark(db, userId, Number(event.params.id));
		if (!deleted) return apiError(404, 'Unbekanntes Lesezeichen');
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const DELETE: RequestHandler = (event) => _bookmarkDeleteHandler(defaultDb, event);
