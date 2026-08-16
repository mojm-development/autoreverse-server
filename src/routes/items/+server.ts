import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { items } from '$lib/server/library/queries';
import { toItemSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { intParam } from '$lib/server/api/validate';

export async function _itemsGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const url = event.url;
		const kind = url.searchParams.get('kind') ?? undefined;
		const q = url.searchParams.get('q') ?? undefined;
		const missingParam = url.searchParams.get('missing');
		const missing = missingParam === null ? undefined : missingParam === 'true';
		const limit = intParam(url, 'limit', { def: 200, min: 1, max: 500 });
		const offset = intParam(url, 'offset', { def: 0, min: 0, max: Number.MAX_SAFE_INTEGER });
		const rows = await items(db, { kind, q, missing, limit, offset, sort: 'title' });
		return json({ items: rows.map(toItemSummary) });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _itemsGetHandler(defaultDb, event);
