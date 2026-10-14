import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { and, eq, sql } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { playlists, playlistEntries } from '$lib/server/db/schema';
import { removeEntry, moveEntry } from '$lib/server/library/playlistEntries';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { readJson } from '$lib/server/api/validate';

async function ownedEntry(db: DrizzleDb, userId: number, playlistId: number, entryId: number) {
	const [playlist] = await db
		.select()
		.from(playlists)
		.where(and(eq(playlists.id, playlistId), eq(playlists.userId, userId)));
	if (!playlist) return { playlist: null, entry: null };
	const [entry] = await db
		.select()
		.from(playlistEntries)
		.where(and(eq(playlistEntries.id, entryId), eq(playlistEntries.playlistId, playlistId)));
	return { playlist, entry: entry ?? null };
}

export async function _playlistEntryDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { playlist, entry } = await ownedEntry(
			db,
			userId,
			Number(event.params.id),
			Number(event.params.entryId)
		);
		if (!playlist) return apiError(404, 'Unbekannte Playlist');
		if (!entry) return apiError(404, 'Unbekannter Eintrag');
		await removeEntry(db, playlist.id, entry.id, entry.position);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _playlistEntryPutHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { playlist, entry } = await ownedEntry(
			db,
			userId,
			Number(event.params.id),
			Number(event.params.entryId)
		);
		if (!playlist) return apiError(404, 'Unbekannte Playlist');
		if (!entry) return apiError(404, 'Unbekannter Eintrag');
		const { position } = await readJson<{ position: number }>(event.request);
		const [{ count }] = await db
			.select({ count: sql<number>`count(*)::int` })
			.from(playlistEntries)
			.where(eq(playlistEntries.playlistId, playlist.id));
		if (position < 1 || position > count)
			return apiError(422, 'position außerhalb des gültigen Bereichs');
		await moveEntry(db, playlist.id, entry.id, entry.position, position);
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const DELETE: RequestHandler = (event) => _playlistEntryDeleteHandler(defaultDb, event);
export const PUT: RequestHandler = (event) => _playlistEntryPutHandler(defaultDb, event);
