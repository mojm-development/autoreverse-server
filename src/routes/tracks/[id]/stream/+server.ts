import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { stat } from 'node:fs/promises';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { track } from '$lib/server/library/queries';
import { rangeResponse } from '$lib/server/streaming/ranges';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

async function streamHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>,
	head: boolean
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const row = await track(db, Number(event.params.id));
		if (!row) return apiError(404, 'Unbekannter Track');
		try {
			await stat(row.path);
		} catch {
			return apiError(410, 'Datei nicht mehr vorhanden');
		}
		return rangeResponse(row.path, event.request.headers.get('range'), head);
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _streamGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	return streamHandler(db, event, false);
}
export async function _streamHeadHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	return streamHandler(db, event, true);
}

export const GET: RequestHandler = (event) => _streamGetHandler(defaultDb, event);
export const HEAD: RequestHandler = (event) => _streamHeadHandler(defaultDb, event);
