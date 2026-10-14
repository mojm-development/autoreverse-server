import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { items, BOOK_SORTS, type SortKey } from '$lib/server/library/queries';
import { toItemSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { intParam } from '$lib/server/api/validate';

export async function _itemsGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const url = event.url;
		const kind = url.searchParams.get('kind') ?? undefined;
		const q = url.searchParams.get('q') ?? undefined;
		const missingParam = url.searchParams.get('missing');
		const missing = missingParam === null ? undefined : missingParam === 'true';
		const limit = intParam(url, 'limit', { def: 200, min: 1, max: 500 });
		const offset = intParam(url, 'offset', { def: 0, min: 0, max: Number.MAX_SAFE_INTEGER });

		// `items()` has always understood these; only this handler pinned them to `title`,
		// so a client could not ask for "newest first" without going through
		// `/library/more` — which answers with raw rows and covers albums and books only.
		//
		// Unlike `/library/more`, an unknown value is refused rather than quietly read as
		// `title`: this is the surface clients are written against, and a sort that silently
		// does something else is a bug that takes a long time to notice.
		const requestedSort = url.searchParams.get('sort') ?? 'title';
		if (!(BOOK_SORTS as readonly string[]).includes(requestedSort)) {
			throw new ApiError(422, 'Unbekannte Sortierung');
		}
		const sort = requestedSort as SortKey;

		const rows = await items(db, {
			kind,
			q,
			missing,
			limit,
			offset,
			sort,
			// Only `played` needs the progress join; for every other order it would cost
			// time and change nothing.
			playedBy: sort === 'played' ? userId : undefined
		});
		return json({ items: rows.map(toItemSummary) });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _itemsGetHandler(defaultDb, event);
