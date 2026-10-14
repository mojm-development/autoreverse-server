import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { previewFeed, FeedFetchError, InvalidFeedError } from '$lib/server/podcasts/store';
import { toIso } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { readJson } from '$lib/server/api/validate';

/** How many episodes a preview carries. Enough to judge a show, not a whole archive. */
const PREVIEW_EPISODES = 15;

export async function _podcastsPreviewPostHandler(
	_db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const { feed_url } = await readJson<{ feed_url?: unknown }>(event.request);
		if (typeof feed_url !== 'string' || feed_url.length < 1)
			return apiError(422, 'feed_url muss eine nicht-leere Zeichenkette sein');

		const parsed = await previewFeed(feed_url);
		return json({
			title: parsed.title,
			description: parsed.description,
			image_url: parsed.imageUrl,
			episodes: parsed.episodes.slice(0, PREVIEW_EPISODES).map((episode) => ({
				title: episode.title,
				published_at: toIso(episode.publishedAt),
				duration: episode.durationSeconds
			}))
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		if (e instanceof FeedFetchError) return apiError(502, e.message);
		if (e instanceof InvalidFeedError) return apiError(422, e.message);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _podcastsPreviewPostHandler(defaultDb, event);
