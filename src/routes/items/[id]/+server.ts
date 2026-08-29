import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser, requireApiAdmin } from '$lib/server/auth/session';
import { item, tracks, chapters } from '$lib/server/library/queries';
import { toItemDetail, toTrackOut, toChapterOut } from '$lib/server/api/serialize';
import { editItem } from '$lib/server/library/metadata';
import { readJson } from '$lib/server/api/validate';
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
			...toItemDetail(row),
			tracks: trackRows.map(toTrackOut),
			chapters: chapterRows.map(toChapterOut)
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

/**
 * Hand-editing an item's metadata. Every field set here is locked against the
 * scanner, which otherwise rewrites it from the files on the next scan; `reset`
 * hands a field back. Verwalter only — this changes what everyone sees.
 */
export async function _itemPatchHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = await requireApiAdmin(event.locals, db);
		const body = await readJson(event.request);
		const result = await editItem(db, userId, Number(event.params.id), body);
		return json({
			...toItemDetail(result.row),
			locked_fields: result.row.lockedFields,
			changed: result.changed,
			batch_id: result.batchId
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _itemGetHandler(defaultDb, event);
export const PATCH: RequestHandler = (event) => _itemPatchHandler(defaultDb, event);
