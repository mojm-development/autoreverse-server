import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item, tracks, chapters } from '$lib/server/library/queries';
import { toItemSummary, toTrackOut, toChapterOut } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _itemGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const row = await item(db, Number(event.params.id));
		if (!row) return apiError(404, 'Unbekanntes Item');
		const [trackRows, chapterRows] = await Promise.all([tracks(db, row.id), chapters(db, row.id)]);
		return json({
			...toItemSummary(row),
			tracks: trackRows.map(toTrackOut),
			chapters: chapterRows.map(toChapterOut)
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _itemGetHandler(defaultDb, event);
