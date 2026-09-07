import type { Handle, HandleServerError, ServerInit } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import { SESSION_COOKIE, tokenFromRequest } from '$lib/server/auth/session';
import { userForToken } from '$lib/server/auth/tokens';
import { ensureFirstAdmin } from '$lib/server/auth/bootstrap';
import { loadConfig, type Config } from '$lib/server/config';
import { refreshAllAndRetain } from '$lib/server/podcasts/retention';
import { relocateLegacyDownloads } from '$lib/server/podcasts/relocate';
import { db } from '$lib/server/db';

export const init: ServerInit = async () => {
	if (env.AUTOREVERSE_AUTO_MIGRATE) {
		await migrate(db, { migrationsFolder: 'drizzle' });
	}
	await ensureFirstAdmin(db, env);
	const config = loadConfig(process.env as Record<string, string | undefined>);
	// Old downloads move before the refresh loop starts, never alongside it: a retention sweep
	// running at the same time would delete or re-download the files being renamed. Not awaited,
	// so a large podcast folder does not hold up the first request.
	void relocateLegacyDownloads(db, config.podcastsDir)
		.then((result) => {
			if (result.moved || result.failed)
				console.log(
					`[podcasts] moved ${result.moved} old downloads into their podcast folder` +
						(result.failed ? `, ${result.failed} could not be moved` : '')
				);
		})
		.catch((e) => console.error('[podcasts] moving old downloads failed', e))
		.finally(() => startPodcastRefresh(config));
};

// Every path through this says something in the log. A loop that starts, runs and finds nothing
// looks exactly like a loop that never started, and the only way to tell them apart after the
// fact is a line per run.
function startPodcastRefresh(config: Config) {
	const hours = config.podcastRefreshHours;
	if (hours <= 0) {
		console.log('[podcasts] automatic refresh is off (AUTOREVERSE_PODCAST_REFRESH_HOURS=0)');
		return;
	}
	const dirs = { coversDir: config.coverDir, podcastsDir: config.podcastsDir };
	let running = false;
	const tick = async () => {
		if (running) {
			console.warn('[podcasts] previous refresh still running, skipping this run');
			return;
		}
		running = true;
		try {
			const run = await refreshAllAndRetain(db, dirs);
			console.log(
				`[podcasts] refreshed ${run.podcasts} feeds: ${run.newEpisodes} new episodes, ` +
					`${run.downloaded} downloaded, ${run.freed} removed, ${run.failed} failed`
			);
			for (const error of run.errors) console.error(`[podcasts] ${error}`);
		} catch (e) {
			console.error('[podcasts] refresh failed', e);
		} finally {
			running = false;
		}
	};
	console.log(`[podcasts] refreshing every ${hours} hour${hours === 1 ? '' : 's'}`);
	const timer = setInterval(tick, hours * 60 * 60 * 1000);
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
