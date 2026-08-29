import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item, tracks, chapters, progress, coverPathFor } from '$lib/server/library/queries';
import { openSession } from '$lib/server/library/playback';
import { toTrackOut, toChapterOut } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _playPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const row = await item(db, Number(event.params.itemId));
		if (!row) return apiError(404, 'Unbekanntes Item');
		const trackRows = await tracks(db, row.id);
		if (row.kind === 'episode' && trackRows.length === 0) {
			return apiError(409, 'Folge ist noch nicht heruntergeladen');
		}
		const existing = await progress(db, userId, row.id);
		const startPosition = existing?.position ?? 0.0;
		const sessionId = await openSession(db, userId, row.id, startPosition);
		const chapterRows = await chapters(db, row.id);
		return json({
			session_id: sessionId,
			kind: row.kind,
			has_cover: Boolean(await coverPathFor(db, row)),
			start_position: startPosition,
			tracks: trackRows.map(toTrackOut),
			chapters: chapterRows.map(toChapterOut)
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const POST: RequestHandler = (event) => _playPostHandler(defaultDb, event);
