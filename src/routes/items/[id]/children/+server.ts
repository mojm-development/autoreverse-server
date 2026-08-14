import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item, children } from '$lib/server/library/queries';
import { toItemSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function itemChildrenGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const parent = await item(db, Number(event.params.id));
		if (!parent) return apiError(404, 'Unbekanntes Item');
		const rows = await children(db, parent.id);
		return json({ items: rows.map(toItemSummary) });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => itemChildrenGetHandler(defaultDb, event);
