import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { refresh, FeedFetchError, InvalidFeedError } from '$lib/server/podcasts/store';
import { retainForPodcast } from '$lib/server/podcasts/retention';
import { loadConfig } from '$lib/server/config';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _podcastRefreshPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const { coverDir, podcastsDir } = loadConfig(process.env as Record<string, string | undefined>);
		const podcast = await refresh(db, Number(event.params.id), { coversDir: coverDir });
		const retention = await retainForPodcast(db, podcast.id, podcastsDir);
		return json({
			id: podcast.id,
			title: podcast.title,
			feed_url: podcast.feedUrl,
			new_episodes: podcast.newEpisodes,
			updated_episodes: podcast.updatedEpisodes,
			downloaded: retention.downloaded,
			freed: retention.freed
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		if (e instanceof FeedFetchError) return apiError(502, e.message);
		if (e instanceof InvalidFeedError) return apiError(422, e.message);
		if (e instanceof Error && e.message === 'not found')
			return apiError(404, 'Unbekannter Podcast');
		throw e;
	}
}

export const POST: RequestHandler = (event) => _podcastRefreshPostHandler(defaultDb, event);
