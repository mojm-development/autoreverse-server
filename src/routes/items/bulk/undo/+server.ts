import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { undoBatch } from '$lib/server/library/bulkMetadata';
import { readJson } from '$lib/server/api/validate';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _itemsBulkUndoPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const body = await readJson<{ batch_id?: unknown }>(event.request);
		const batchId = body?.batch_id;
		if (typeof batchId !== 'string' || batchId === '') {
			return apiError(422, 'batch_id fehlt');
		}
		return json(await undoBatch(db, batchId));
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _itemsBulkUndoPostHandler(defaultDb, event);
