import type { Handle, HandleServerError, ServerInit } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import { SESSION_COOKIE, tokenFromRequest } from '$lib/server/auth/session';
import { userForToken } from '$lib/server/auth/tokens';
import { ensureFirstAdmin } from '$lib/server/auth/bootstrap';
import { db } from '$lib/server/db';

export const init: ServerInit = async () => {
	if (env.AUTOREVERSE_AUTO_MIGRATE) {
		await migrate(db, { migrationsFolder: 'drizzle' });
	}
	await ensureFirstAdmin(db, env);
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

export const handleError: HandleServerError = () => {
	return { message: 'Interner Serverfehler', detail: 'Interner Serverfehler' };
};
