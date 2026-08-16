import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import {
	searchDirectory,
	DirectorySearchError,
	DEFAULT_LIMIT,
	DEFAULT_COUNTRY
} from '$lib/server/podcasts/directory';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _podcastsSearchGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const q = event.url.searchParams.get('q') ?? '';
		if (q.length < 1 || q.length > 200) throw new ApiError(422, 'q muss 1–200 Zeichen haben');
		const limit = Math.min(
			50,
			Math.max(1, Number(event.url.searchParams.get('limit') ?? DEFAULT_LIMIT))
		);
		const country = event.url.searchParams.get('country') ?? DEFAULT_COUNTRY;
		const results = await searchDirectory(q, { limit, country });
		return json({
			results: results.map((r) => ({
				name: r.name,
				author: r.author,
				feed_url: r.feedUrl,
				artwork_url: r.artworkUrl,
				episode_count: r.episodeCount,
				genre: r.genre
			}))
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		if (e instanceof DirectorySearchError) return apiError(502, e.message);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _podcastsSearchGetHandler(defaultDb, event);
