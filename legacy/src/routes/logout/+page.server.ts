import { redirect, type Actions } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebUser, SESSION_COOKIE } from '$lib/server/auth/session';
import { revokeToken } from '$lib/server/auth/tokens';

export const actions: Actions = {
	default: async ({ locals, cookies }) => {
		requireWebUser(locals);
		if (locals.token) await revokeToken(db, locals.token);
		cookies.delete(SESSION_COOKIE, { path: '/' });
		throw redirect(303, '/login');
	}
};
