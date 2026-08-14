import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { listFavoriteItems, listFavoriteTracks } from '$lib/server/library/favorites';
import { toItemSummary, toTrackSummary } from '$lib/server/api/serialize';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { type items as ItemsType, type tracks as TracksType } from '$lib/server/db/schema';

type TrackResult = {
	track: typeof TracksType.$inferSelect;
	item: typeof ItemsType.$inferSelect;
};

export async function favoritesGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const [items, tracks] = await Promise.all([
			listFavoriteItems(db, userId),
			listFavoriteTracks(db, userId)
		]);
		return json({
			items: items.map(toItemSummary),
			tracks: tracks.map((r: TrackResult) =>
				toTrackSummary({
					...r.track,
					item_id: r.item.id,
					item_title: r.item.title,
					item_kind: r.item.kind
				})
			)
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => favoritesGetHandler(defaultDb, event);
