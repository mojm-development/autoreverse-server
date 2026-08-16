import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { refresh, FeedFetchError, InvalidFeedError } from '$lib/server/podcasts/store';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _podcastRefreshPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const podcast = await refresh(db, Number(event.params.id));
		return json({
			id: podcast.id,
			title: podcast.title,
			feed_url: podcast.feedUrl,
			new_episodes: 0,
			updated_episodes: 0
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		if (e instanceof FeedFetchError) return apiError(502, e.message);
		if (e instanceof InvalidFeedError) return apiError(422, e.message);
		return apiError(404, 'Unbekannter Podcast');
	}
}

export const POST: RequestHandler = (event) => _podcastRefreshPostHandler(defaultDb, event);
