import type { Handle, ServerInit } from '@sveltejs/kit';
import { SESSION_COOKIE, tokenFromRequest } from '$lib/server/auth/session';
import { userForToken } from '$lib/server/auth/tokens';
import { ensureFirstAdmin } from '$lib/server/auth/bootstrap';
import { db } from '$lib/server/db';

export const init: ServerInit = async () => {
	await ensureFirstAdmin(db, process.env as Record<string, string | undefined>);
};

export const handle: Handle = async ({ event, resolve }) => {
	const cookieToken = event.cookies.get(SESSION_COOKIE) ?? null;
	const token = tokenFromRequest(event.request, cookieToken);
	if (token) {
		event.locals.userId = await userForToken(db, token);
		event.locals.token = event.locals.userId !== null ? token : null;
	} else {
		event.locals.userId = null;
		event.locals.token = null;
	}
	return resolve(event);
};
