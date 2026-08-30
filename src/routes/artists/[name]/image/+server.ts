import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { resolve, relative } from 'node:path';
import { stat, readFile } from 'node:fs/promises';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser, requireApiAdmin } from '$lib/server/auth/session';
import { loadConfig } from '$lib/server/config';
import {
	chosenCover,
	fallbackCovers,
	storeImage,
	imageContentType
} from '$lib/server/library/artistCovers';
import { items as itemsTable } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

function isInside(path: string, root: string): boolean {
	const rel = relative(resolve(root), resolve(path));
	return rel !== '' && !rel.startsWith('..') && !resolve(rel).includes('..');
}

// Das Cover eines Albums unter der Adresse des Interpreten ausliefern. Kein
// Weiterleiten auf `/items/{id}/cover`: Ein Redirect zwänge jeden Client zu einem
// zweiten Aufruf, und die Adresse des Bildes soll die des Interpreten bleiben —
// wechselt seine Auswahl, wechselt der Inhalt, nicht der Ort.
async function itemCover(db: DrizzleDb, itemId: number): Promise<Response> {
	const [row] = await db
		.select({ coverPath: itemsTable.coverPath })
		.from(itemsTable)
		.where(eq(itemsTable.id, itemId));
	if (!row?.coverPath) return apiError(404, 'Kein Bild');

	let stats;
	try {
		stats = await stat(row.coverPath);
	} catch {
		return apiError(404, 'Kein Bild');
	}
	const data = await readFile(row.coverPath);
	return new Response(data, {
		headers: {
			'content-type': imageContentType(row.coverPath),
			'content-length': String(stats.size),
			'cache-control': 'private, max-age=300'
		}
	});
}

export async function _artistImageGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const artist = event.params.name ?? '';
		const cover = await chosenCover(db, artist);

		// Ein Interpret hat sein Bild aus einer von drei Quellen, und diese Route
		// liefert es aus allen dreien statt nur aus der ersten. Vorher gab sie bei
		// einer **Albumauswahl** 404 zurück: Die Weboberfläche löst die Auswahl in
		// ihrem Seitenlader selbst auf und merkte davon nichts, aber ein Client, der
		// nur diese Route kennt, sah eine getroffene Auswahl schlicht nicht.
		if (!cover?.imagePath) {
			const itemId = cover?.itemId ?? (await fallbackCovers(db)).get(artist);
			if (!itemId) return apiError(404, 'Kein Bild');
			return itemCover(db, itemId);
		}

		const config = loadConfig(process.env as Record<string, string | undefined>);
		if (!isInside(cover.imagePath, config.artistsDir)) return apiError(404, 'Kein Bild');

		let stats;
		try {
			stats = await stat(cover.imagePath);
		} catch {
			return apiError(404, 'Kein Bild');
		}
		const data = await readFile(cover.imagePath);
		return new Response(data, {
			headers: {
				'content-type': imageContentType(cover.imagePath),
				'content-length': String(stats.size),
				'cache-control': 'private, max-age=300'
			}
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _artistImagePostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const artist = event.params.name ?? '';
		if (!artist) throw new ApiError(422, 'Kein Interpret angegeben');

		let form: FormData;
		try {
			form = await event.request.formData();
		} catch {
			throw new ApiError(422, 'Ungültiger Upload');
		}
		const file = form.get('image');
		if (!(file instanceof File)) throw new ApiError(422, 'Keine Bilddatei im Feld "image"');

		const config = loadConfig(process.env as Record<string, string | undefined>);
		await storeImage(
			db,
			artist,
			{ type: file.type, size: file.size, bytes: new Uint8Array(await file.arrayBuffer()) },
			config.artistsDir
		);
		return json({ artist }, { status: 201 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _artistImageGetHandler(defaultDb, event);
export const POST: RequestHandler = (event) => _artistImagePostHandler(defaultDb, event);
