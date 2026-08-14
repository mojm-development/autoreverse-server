import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { searchItems, searchTracks } from '$lib/server/library/queries';
import { toItemSummary, toTrackSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function searchGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'url'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const q = event.url.searchParams.get('q') ?? '';
		if (q.length < 1 || q.length > 200) return apiError(422, 'q muss 1–200 Zeichen haben');
		const limitParam = Number(event.url.searchParams.get('limit') ?? 20);
		const limit = Math.min(100, Math.max(1, limitParam));

		const [books, albums, podcasts, tracks] = await Promise.all([
			searchItems(db, q, ['book'], limit),
			searchItems(db, q, ['album'], limit),
			searchItems(db, q, ['podcast', 'episode'], limit),
			searchTracks(db, q, limit)
		]);
		return json({
			books: books.map(toItemSummary),
			albums: albums.map(toItemSummary),
			podcasts: podcasts.map(toItemSummary),
			tracks: tracks.map((track) => toTrackSummary(track as Parameters<typeof toTrackSummary>[0]))
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => searchGetHandler(defaultDb, event);
