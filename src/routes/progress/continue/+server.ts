import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { continueListening } from '$lib/server/library/queries';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _progressContinueGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const limit = Math.min(100, Math.max(1, Number(event.url.searchParams.get('limit') ?? 20)));
		const rows = await continueListening(db, userId, limit);
		return json({
			items: rows.map((r: { id: number; title: string; kind: string; position: number }) => ({
				id: r.id,
				title: r.title,
				kind: r.kind,
				position: r.position
			}))
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => _progressContinueGetHandler(defaultDb, event);
