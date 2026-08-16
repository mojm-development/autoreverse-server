import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { scanState, snapshot } from '$lib/server/admin/scanState';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { _toWire } from '../+server';

export async function _scanCancelPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		if (!scanState.running) throw new ApiError(409, 'Es läuft gerade kein Scan');
		scanState.cancelRequested = true;
		return json(_toWire(snapshot()), { status: 202 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _scanCancelPostHandler(defaultDb, event);
