import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { editTrack } from '$lib/server/library/metadata';
import { toTrackOut } from '$lib/server/api/serialize';
import { readJson } from '$lib/server/api/validate';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

/**
 * Correcting a track's own metadata — its title, mostly, which is what a badly
 * tagged rip gets wrong. Locked against the scanner like an item's fields.
 */
export async function _trackPatchHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = await requireApiAdmin(event.locals, db);
		const body = await readJson(event.request);
		const result = await editTrack(db, userId, Number(event.params.id), body);
		return json({
			...toTrackOut(result.row),
			disc: result.row.disc ?? null,
			locked_fields: result.row.lockedFields,
			changed: result.changed,
			batch_id: result.batchId
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const PATCH: RequestHandler = (event) => _trackPatchHandler(defaultDb, event);
