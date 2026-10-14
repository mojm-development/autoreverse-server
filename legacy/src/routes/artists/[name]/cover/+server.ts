import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin, requireApiUser } from '$lib/server/auth/session';
import { selectAlbum, clearCover, chosenCover } from '$lib/server/library/artistCovers';
import { readJson } from '$lib/server/api/validate';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _artistCoverGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const artist = event.params.name ?? '';
		if (!artist) throw new ApiError(422, 'Kein Interpret angegeben');
		const cover = await chosenCover(db, artist);
		// `item_id` und `has_image` schließen einander aus (siehe die Prüfbedingung
		// `artist_cover_xor` der Tabelle). Beide Felder stehen trotzdem da, weil der
		// Client sonst aus einer fehlenden ID nicht schließen könnte, ob gar nichts
		// gewählt ist oder eine Bilddatei liegt.
		return json({
			artist,
			item_id: cover?.itemId ?? null,
			has_image: Boolean(cover?.imagePath)
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _artistCoverPutHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const artist = event.params.name ?? '';
		if (!artist) throw new ApiError(422, 'Kein Interpret angegeben');

		const body = await readJson<{ item_id?: unknown }>(event.request);
		const itemId = Number(body.item_id);
		if (!Number.isInteger(itemId) || itemId <= 0) throw new ApiError(422, 'Ungültige Album-ID');

		await selectAlbum(db, artist, itemId);
		return json({ artist, item_id: itemId });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _artistCoverDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const artist = event.params.name ?? '';
		if (!artist) throw new ApiError(422, 'Kein Interpret angegeben');
		await clearCover(db, artist);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _artistCoverGetHandler(defaultDb, event);
export const PUT: RequestHandler = (event) => _artistCoverPutHandler(defaultDb, event);
export const DELETE: RequestHandler = (event) => _artistCoverDeleteHandler(defaultDb, event);
