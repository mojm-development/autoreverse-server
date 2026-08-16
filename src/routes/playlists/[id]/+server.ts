import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { and, eq, sql } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { playlists, playlistEntries } from '$lib/server/db/schema';
import { listEntries } from '$lib/server/library/playlistEntries';
import { item, track } from '$lib/server/library/queries';
import { toItemSummary, toTrackSummary, toIso } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { readJson } from '$lib/server/api/validate';

async function ownedPlaylist(db: DrizzleDb, userId: number, id: number) {
	const [row] = await db
		.select()
		.from(playlists)
		.where(and(eq(playlists.id, id), eq(playlists.userId, userId)));
	return row ?? null;
}

export async function _playlistGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const playlist = await ownedPlaylist(db, userId, Number(event.params.id));
		if (!playlist) return apiError(404, 'Unbekannte Playlist');
		const entries = await listEntries(db, playlist.id);
		const serializedEntries = await Promise.all(
			entries.map(async (e) => {
				if (e.itemId)
					return {
						id: e.id,
						position: e.position,
						item: toItemSummary(await item(db, e.itemId)),
						track: null
					};
				const t = await track(db, e.trackId!);
				if (!t) return { id: e.id, position: e.position, item: null, track: null };
				const parentItem = await item(db, t.itemId);
				return {
					id: e.id,
					position: e.position,
					item: null,
					track: toTrackSummary({
						...t,
						item_id: t.itemId,
						item_title: parentItem?.title ?? '',
						item_kind: parentItem?.kind ?? ''
					})
				};
			})
		);
		return json({
			id: playlist.id,
			name: playlist.name,
			created_at: toIso(playlist.createdAt),
			entries: serializedEntries
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _playlistPatchHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const playlist = await ownedPlaylist(db, userId, Number(event.params.id));
		if (!playlist) return apiError(404, 'Unbekannte Playlist');
		const { name } = await readJson<{ name?: unknown }>(event.request);
		if (typeof name !== 'string' || name.length < 1 || name.length > 100)
			return apiError(422, 'name muss 1–100 Zeichen haben');
		const [row] = await db
			.update(playlists)
			.set({ name })
			.where(eq(playlists.id, playlist.id))
			.returning();
		const [{ count }] = await db
			.select({ count: sql<number>`count(*)::int` })
			.from(playlistEntries)
			.where(eq(playlistEntries.playlistId, row.id));
		return json({
			id: row.id,
			name: row.name,
			created_at: toIso(row.createdAt),
			entry_count: count
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _playlistDeleteHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const playlist = await ownedPlaylist(db, userId, Number(event.params.id));
		if (!playlist) return apiError(404, 'Unbekannte Playlist');
		await db.delete(playlists).where(eq(playlists.id, playlist.id));
		return new Response(null, { status: 204 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => _playlistGetHandler(defaultDb, event);
export const PATCH: RequestHandler = (event) => _playlistPatchHandler(defaultDb, event);
export const DELETE: RequestHandler = (event) => _playlistDeleteHandler(defaultDb, event);
