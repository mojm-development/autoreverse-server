import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item } from '$lib/server/library/queries';
import { savePosition } from '$lib/server/library/playback';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _progressPutHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const row = await item(db, Number(event.params.itemId));
		if (!row) return apiError(404, 'Unbekanntes Item');
		const { position, finished = false } = await event.request.json();
		await savePosition(db, userId, row.id, position, finished);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const PUT: RequestHandler = (event) => _progressPutHandler(defaultDb, event);
// `navigator.sendBeacon` (used by the beforeunload handler in
// src/routes/+layout.svelte to flush progress on tab/page close) can only
// ever issue a POST request — there is no way to make it send PUT. Without
// this alias, that beacon silently 404s on every real navigation-away (fire-
// and-forget, so nothing ever surfaced the failure) and in-progress position
// was never actually persisted on reload/tab-close. Caught by the Task 41
// end-to-end smoke test (tests/e2e/smoke.e2e.ts), which is exactly the class
// of bug that test exists to catch.
export const POST: RequestHandler = (event) => _progressPutHandler(defaultDb, event);
