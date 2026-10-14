import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item } from '$lib/server/library/queries';
import { savePosition } from '$lib/server/library/playback';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { readJson } from '$lib/server/api/validate';

export async function _progressPutHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const row = await item(db, Number(event.params.itemId));
		if (!row) return apiError(404, 'Unbekanntes Item');
		const { position, finished = false } = await readJson<{
			position?: unknown;
			finished?: boolean;
		}>(event.request);
		if (typeof position !== 'number' || !Number.isFinite(position))
			return apiError(422, 'position muss eine Zahl sein');
		await savePosition(db, userId, row.id, position, finished);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const PUT: RequestHandler = (event) => _progressPutHandler(defaultDb, event);
export const POST: RequestHandler = (event) => _progressPutHandler(defaultDb, event);
