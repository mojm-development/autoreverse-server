import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { unsubscribe } from '$lib/server/podcasts/store';
import { loadConfig } from '$lib/server/config';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function podcastDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const config = loadConfig(process.env as Record<string, string | undefined>);
		const report = await unsubscribe(db, Number(event.params.id), config.podcastsDir);
		return json({
			episodes: report.episodes,
			files_deleted: report.filesDeleted,
			files_kept: report.filesKept
		}); // 200, not 204
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		return apiError(404, 'Unbekannter Podcast');
	}
}

export const DELETE: RequestHandler = (event) => podcastDeleteHandler(defaultDb, event);
