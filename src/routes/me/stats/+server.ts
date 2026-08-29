import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { listeningStats } from '$lib/server/library/queries';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { intParam } from '$lib/server/api/validate';

export async function _meStatsGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const days = intParam(event.url, 'days', { def: 30, min: 1, max: 365 });
		return json(await listeningStats(db, userId, days));
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _meStatsGetHandler(defaultDb, event);
