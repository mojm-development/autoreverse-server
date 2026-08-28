import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item } from '$lib/server/library/queries';
import { addItemFavorite, removeItemFavorite } from '$lib/server/library/favorites';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _favoriteItemPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const row = await item(db, Number(event.params.id));
		if (!row) return apiError(404, 'Unbekanntes Item');
		await addItemFavorite(db, userId, row.id);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export async function _favoriteItemDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		await removeItemFavorite(db, userId, Number(event.params.id));
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const POST: RequestHandler = (event) => _favoriteItemPostHandler(defaultDb, event);
export const DELETE: RequestHandler = (event) => _favoriteItemDeleteHandler(defaultDb, event);
