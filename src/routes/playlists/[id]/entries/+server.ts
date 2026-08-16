import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { and, eq } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { playlists, playlistEntries } from '$lib/server/db/schema';
import { appendEntry } from '$lib/server/library/playlistEntries';
import { item, track } from '$lib/server/library/queries';
import { toItemSummary, toTrackSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _playlistEntriesPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const [playlist] = await db
			.select()
			.from(playlists)
			.where(and(eq(playlists.id, Number(event.params.id)), eq(playlists.userId, userId)));
		if (!playlist) return apiError(404, 'Unbekannte Playlist');
		const { item_id, track_id } = await event.request.json();
		if ((item_id == null) === (track_id == null))
			return apiError(422, 'Genau eines von item_id/track_id angeben');
		if (item_id != null && !(await item(db, item_id))) return apiError(404, 'Unbekanntes Item');
		if (track_id != null && !(await track(db, track_id))) return apiError(404, 'Unbekannter Track');
		const entryId = await appendEntry(db, playlist.id, {
			itemId: item_id ?? undefined,
			trackId: track_id ?? undefined
		});
		const [entry] = await db.select().from(playlistEntries).where(eq(playlistEntries.id, entryId));
		if (entry.itemId) {
			return json({
				id: entry.id,
				position: entry.position,
				item: toItemSummary(await item(db, entry.itemId)),
				track: null
			});
		}
		const t = await track(db, entry.trackId!);
		if (!t) {
			return json({
				id: entry.id,
				position: entry.position,
				item: null,
				track: null
			});
		}
		const parentItem = await item(db, t.itemId);
		return json({
			id: entry.id,
			position: entry.position,
			item: null,
			track: toTrackSummary({
				...t,
				item_id: t.itemId,
				item_title: parentItem?.title ?? '',
				item_kind: parentItem?.kind ?? ''
			})
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const POST: RequestHandler = (event) => _playlistEntriesPostHandler(defaultDb, event);
