import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import {
	downloadEpisode,
	EpisodeNotDownloadableError,
	EpisodeFetchError,
	EpisodeStorageError
} from '$lib/server/podcasts/download';
import { loadConfig } from '$lib/server/config';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _episodeDownloadPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const config = loadConfig(process.env as Record<string, string | undefined>);
		const result = await downloadEpisode(db, Number(event.params.id), config.podcastsDir);
		return json({ track_id: result.trackId, duration: result.duration });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		if (e instanceof EpisodeNotDownloadableError) return apiError(422, e.message);
		if (e instanceof EpisodeFetchError) return apiError(502, e.message);
		if (e instanceof EpisodeStorageError) return apiError(507, e.message);
		return apiError(404, 'Unbekannte Folge');
	}
}

export const POST: RequestHandler = (event) => _episodeDownloadPostHandler(defaultDb, event);
