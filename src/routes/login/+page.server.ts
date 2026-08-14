import { fail, redirect, type Actions } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { performLogin } from '$lib/server/auth/login';
import { loginThrottle, loginHashSemaphore } from '$lib/server/auth/semaphore';
import { SESSION_COOKIE } from '$lib/server/auth/session';
import { ApiError } from '$lib/server/api/errors';

export const load = async ({ locals }) => {
	if (locals.userId !== null) throw redirect(303, '/library');
	return {};
};

export const actions: Actions = {
	default: async ({ request, cookies, url }) => {
		const data = await request.formData();
		const name = String(data.get('name') ?? '');
		const password = String(data.get('password') ?? '');
		try {
			const token = await performLogin(db, loginThrottle, loginHashSemaphore, name, password);
			cookies.set(SESSION_COOKIE, token, {
				path: '/',
				httpOnly: true,
				sameSite: 'lax',
				secure: url.protocol === 'https:'
			});
		} catch (e: unknown) {
			if (e instanceof ApiError) {
				return fail(e.status, { message: e.detail });
			}
			if (e instanceof Error) {
				return fail(500, { message: e.message });
			}
			return fail(500, { message: 'Anmeldung fehlgeschlagen' });
		}
		throw redirect(303, '/library');
	}
};
