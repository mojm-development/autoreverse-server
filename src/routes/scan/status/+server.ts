import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { snapshot } from '$lib/server/admin/scanState';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { _toWire } from '../+server';

export async function _scanStatusGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db); // last_error may contain filesystem paths — Admin-only, same as POST /scan
		return json(_toWire(snapshot()));
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _scanStatusGetHandler(defaultDb, event);
