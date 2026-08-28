import type { Handle, HandleServerError, ServerInit } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import { SESSION_COOKIE, tokenFromRequest } from '$lib/server/auth/session';
import { userForToken } from '$lib/server/auth/tokens';
import { ensureFirstAdmin } from '$lib/server/auth/bootstrap';
import { db } from '$lib/server/db';

export const init: ServerInit = async () => {
	// Opt-in, not default: dev databases synced via `db:push` have no migration
	// history, so running the migration journal against them would fail. The
	// Docker image sets this — a pulled image must work against a fresh Postgres.
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

// Ruling 8's `{detail: ...}` shape must hold on every path, including ones no
// route handler ever catches (uncaught exceptions, framework-level errors) —
// otherwise those fall back to SvelteKit's default `{"message": ...}` body
// with no `detail` at all. `message` is kept alongside it (App.Error requires
// it) — it's what the default HTML error page renders for page-load errors —
// but every JSON API response now carries `detail` too, which is what C-2 cares about.
export const handleError: HandleServerError = () => {
	return { message: 'Interner Serverfehler', detail: 'Interner Serverfehler' };
};
