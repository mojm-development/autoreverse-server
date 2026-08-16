import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { items } from '$lib/server/library/queries';
import { toItemSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

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
		const limit = Math.min(500, Math.max(1, Number(url.searchParams.get('limit') ?? 200)));
		const offset = Math.max(0, Number(url.searchParams.get('offset') ?? 0));
		const rows = await items(db, { kind, q, missing, limit, offset, sort: 'title' });
		return json({ items: rows.map(toItemSummary) });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _itemsGetHandler(defaultDb, event);
