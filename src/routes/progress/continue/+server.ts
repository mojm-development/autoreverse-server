import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { continueListening } from '$lib/server/library/queries';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { intParam } from '$lib/server/api/validate';

export async function _progressContinueGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const limit = intParam(event.url, 'limit', { def: 20, min: 1, max: 100 });
		const rows = await continueListening(db, userId, limit);
		// `duration` and `has_cover` come free: `continueListening` already sums the track
		// durations via a LATERAL join and selects `cover_path`. Without them a client has
		// to fetch the whole item just to draw a progress bar — for the 2000-track radio-play
		// compilation in the wild that is ~390 KB per tile, and fetching several in parallel
		// starves the cover requests. Both fields are additive; older clients ignore them.
		return json({
			items: rows.map(
				(r: {
					id: number;
					title: string;
					kind: string;
					position: number;
					duration: number;
					cover_path: string | null;
				}) => ({
					id: r.id,
					title: r.title,
					kind: r.kind,
					position: r.position,
					duration: r.duration,
					has_cover: Boolean(r.cover_path)
				})
			)
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => _progressContinueGetHandler(defaultDb, event);
