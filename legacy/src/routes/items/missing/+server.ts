import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { deleteMissing } from '$lib/server/library/queries';
import { scanState } from '$lib/server/admin/scanState';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _itemsMissingDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		if (scanState.running) throw new ApiError(409, 'Es läuft gerade ein Scan');
		const removed = await deleteMissing(db);
		return json({ removed });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const DELETE: RequestHandler = (event) => _itemsMissingDeleteHandler(defaultDb, event);
