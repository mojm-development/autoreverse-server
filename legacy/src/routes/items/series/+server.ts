import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { planSeries } from '$lib/server/library/bulkMetadata';
import { readJson } from '$lib/server/api/validate';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

/**
 * Series and volume numbers for a selection of books: 'detect' applies the
 * scanner's own rules on demand, 'assign' numbers a chosen series through.
 * Dry run by default, undoable by batch id, like every other bulk write.
 */
export async function _itemsSeriesPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		const userId = await requireApiAdmin(event.locals, db);
		return json(await planSeries(db, userId, await readJson(event.request)));
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _itemsSeriesPostHandler(defaultDb, event);
