import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser, SESSION_COOKIE } from '$lib/server/auth/session';
import { revokeToken } from '$lib/server/auth/tokens';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _authLogoutPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request' | 'cookies'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const authorization = event.request.headers.get('authorization') ?? '';
		const token = authorization.split(' ')[1] ?? event.locals.token;
		if (token) await revokeToken(db, token);
		event.cookies.delete(SESSION_COOKIE, { path: '/' });
		return new Response(null, { status: 204 });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
}

export const POST: RequestHandler = (event) => _authLogoutPostHandler(defaultDb, event);
