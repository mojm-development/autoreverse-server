import { json, type RequestHandler } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { performLogin } from '$lib/server/auth/login';
import { loginThrottle, loginHashSemaphore } from '$lib/server/auth/semaphore';
import { ApiError } from '$lib/server/api/errors';
import { apiError } from '$lib/server/api/error';

export const POST: RequestHandler = async ({ request }) => {
	const { name, password } = await request.json();
	try {
		const token = await performLogin(db, loginThrottle, loginHashSemaphore, name, password);
		return json({ token });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
};
