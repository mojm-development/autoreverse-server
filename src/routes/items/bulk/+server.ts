import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { bulkEdit } from '$lib/server/library/bulkMetadata';
import { readJson } from '$lib/server/api/validate';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

/**
 * Editing many items in one go. Defaults to a dry run: the answer is the diff the
 * apply would write, so nothing changes before someone has seen it. Pass
 * `dry_run: false` to actually write, and keep the `batch_id` — it is what undoes it.
 */
export async function _itemsBulkPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		const userId = await requireApiAdmin(event.locals, db);
		const result = await bulkEdit(db, userId, await readJson(event.request));
		return json(result);
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _itemsBulkPostHandler(defaultDb, event);
