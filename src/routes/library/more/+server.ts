import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import {
	items,
	itemDurations,
	BOOK_SORTS,
	PAGE_SIZE,
	type SortKey
} from '$lib/server/library/queries';
import { intParam } from '$lib/server/api/validate';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

const KINDS = ['album', 'book'];

export async function _libraryMoreGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const url = event.url;
		const kind = url.searchParams.get('kind') ?? '';
		if (!KINDS.includes(kind)) throw new ApiError(422, 'Unbekannte Art');

		const requestedSort = url.searchParams.get('sort') ?? 'title';
		const sort = (BOOK_SORTS as readonly string[]).includes(requestedSort)
			? (requestedSort as SortKey)
			: 'title';
		const q = url.searchParams.get('q') || undefined;
		const missing = url.searchParams.get('missing') === 'true';
		const offset = intParam(url, 'offset', { def: 0, min: 0, max: Number.MAX_SAFE_INTEGER });
		const limit = intParam(url, 'limit', { def: PAGE_SIZE, min: 1, max: PAGE_SIZE });

		const rows = await items(db, { kind, sort, q, missing, limit, offset });
		const durations = await itemDurations(
			db,
			rows.map((r) => r.id)
		);
		return json({ items: rows, durations, hasMore: rows.length === limit });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _libraryMoreGetHandler(defaultDb, event);
