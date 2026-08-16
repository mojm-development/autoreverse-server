import type { Handle, HandleServerError, ServerInit } from '@sveltejs/kit';
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

// Ruling 8's `{detail: ...}` shape must hold on every path, including ones no
// route handler ever catches (uncaught exceptions, framework-level errors) —
// otherwise those fall back to SvelteKit's default `{"message": ...}` body
// with no `detail` at all. `message` is kept alongside it (App.Error requires
// it) — it's what the default HTML error page renders for page-load errors —
// but every JSON API response now carries `detail` too, which is what C-2 cares about.
export const handleError: HandleServerError = () => {
	return { message: 'Interner Serverfehler', detail: 'Interner Serverfehler' };
};
