import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { subscribe, FeedFetchError, InvalidFeedError } from '$lib/server/podcasts/store';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { readJson } from '$lib/server/api/validate';
import { loadConfig } from '$lib/server/config';
import { retainForPodcast } from '$lib/server/podcasts/retention';

export async function _podcastsPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const { feed_url } = await readJson<{ feed_url?: unknown }>(event.request);
		if (typeof feed_url !== 'string' || feed_url.length < 1)
			return apiError(422, 'feed_url muss eine nicht-leere Zeichenkette sein');
		const { coverDir, podcastsDir } = loadConfig(process.env as Record<string, string | undefined>);
		const podcast = await subscribe(db, feed_url, { coversDir: coverDir });
		void retainForPodcast(db, podcast.id, podcastsDir);
		return json({
			id: podcast.id,
			title: podcast.title,
			feed_url: podcast.feedUrl,
			new_episodes: podcast.newEpisodes,
			updated_episodes: podcast.updatedEpisodes
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		if (e instanceof FeedFetchError) return apiError(502, e.message);
		if (e instanceof InvalidFeedError) return apiError(422, e.message);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _podcastsPostHandler(defaultDb, event);
