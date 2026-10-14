import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { performLogin } from '$lib/server/auth/login';
import { loginThrottle, loginHashSemaphore } from '$lib/server/auth/semaphore';
import { ApiError } from '$lib/server/api/errors';
import { apiError } from '$lib/server/api/error';
import { readJson } from '$lib/server/api/validate';

export async function _authLoginPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'request'>
): Promise<Response> {
	try {
		const { name, password } = await readJson<{ name: string; password: string }>(event.request);
		const token = await performLogin(db, loginThrottle, loginHashSemaphore, name, password);
		return json({ token });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
}

export const POST: RequestHandler = (event) => _authLoginPostHandler(defaultDb, event);
