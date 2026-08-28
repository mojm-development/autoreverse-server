import type { Handle, HandleServerError, ServerInit } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import { SESSION_COOKIE, tokenFromRequest } from '$lib/server/auth/session';
import { userForToken } from '$lib/server/auth/tokens';
import { ensureFirstAdmin } from '$lib/server/auth/bootstrap';
import { loadConfig } from '$lib/server/config';
import { refreshAllAndRetain } from '$lib/server/podcasts/retention';
import { db } from '$lib/server/db';

export const init: ServerInit = async () => {
	if (env.AUTOREVERSE_AUTO_MIGRATE) {
		await migrate(db, { migrationsFolder: 'drizzle' });
	}
	await ensureFirstAdmin(db, env);
	startPodcastRefresh();
};

function startPodcastRefresh() {
	const config = loadConfig(process.env as Record<string, string | undefined>);
	if (config.podcastRefreshHours <= 0) return;
	const dirs = { coversDir: config.coverDir, podcastsDir: config.podcastsDir };
	let running = false;
	const tick = async () => {
		if (running) return;
		running = true;
		try {
			await refreshAllAndRetain(db, dirs);
		} catch (e) {
			console.error('[podcasts] refresh failed', e);
		} finally {
			running = false;
		}
	};
	const timer = setInterval(tick, config.podcastRefreshHours * 60 * 60 * 1000);
	timer.unref?.();
	void tick();
}

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
