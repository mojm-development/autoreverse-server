import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { closeSession } from '$lib/server/library/playback';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function sessionClosePostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { position } = await event.request.json();
		const session = await closeSession(db, userId, event.params.sessionId!, position);
		if (!session) return apiError(404, 'Unbekannte Sitzung');
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const POST: RequestHandler = (event) => sessionClosePostHandler(defaultDb, event);
