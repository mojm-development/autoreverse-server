import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { track } from '$lib/server/library/queries';
import { addTrackFavorite, removeTrackFavorite } from '$lib/server/library/favorites';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function favoriteTrackPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const row = await track(db, Number(event.params.id));
		if (!row) return apiError(404, 'Unbekannter Track');
		await addTrackFavorite(db, userId, row.id);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export async function favoriteTrackDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		await removeTrackFavorite(db, userId, Number(event.params.id));
		return new Response(null, { status: 204 }); // idempotent, no 404 even if never favorited
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const POST: RequestHandler = (event) => favoriteTrackPostHandler(defaultDb, event);
export const DELETE: RequestHandler = (event) => favoriteTrackDeleteHandler(defaultDb, event);
