import type { RequestHandler } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { revokeToken } from '$lib/server/auth/tokens';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export const POST: RequestHandler = async ({ locals, request }) => {
	try {
		requireApiUser(locals);
		const authorization = request.headers.get('authorization') ?? '';
		const token = authorization.split(' ')[1] ?? locals.token;
		if (token) await revokeToken(db, token);
		return new Response(null, { status: 204 });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
};
