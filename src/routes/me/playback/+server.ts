import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import {
	getPreferences,
	setPreferences,
	SPEED_MIN,
	SPEED_MAX,
	SKIP_MIN,
	SKIP_MAX
} from '$lib/server/auth/preferences';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

function serialize(p: { playbackSpeed: number; skipBack: number; skipForward: number }) {
	return { playback_speed: p.playbackSpeed, skip_back: p.skipBack, skip_forward: p.skipForward };
}

export async function playbackGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		return json(serialize(await getPreferences(db, userId)));
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function playbackPutHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { playback_speed, skip_back, skip_forward } = await event.request.json();
		for (const [value, min, max, name] of [
			[playback_speed, SPEED_MIN, SPEED_MAX, 'playback_speed'],
			[skip_back, SKIP_MIN, SKIP_MAX, 'skip_back'],
			[skip_forward, SKIP_MIN, SKIP_MAX, 'skip_forward']
		] as const) {
			if (typeof value !== 'number' || value < min || value > max)
				return apiError(422, `${name} außerhalb des gültigen Bereichs`);
		}
		return json(
			serialize(await setPreferences(db, userId, playback_speed, skip_back, skip_forward))
		);
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => playbackGetHandler(defaultDb, event);
export const PUT: RequestHandler = (event) => playbackPutHandler(defaultDb, event);
