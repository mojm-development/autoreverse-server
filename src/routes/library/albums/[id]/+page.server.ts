import { error } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { item, tracks } from '$lib/server/library/queries';
import { isItemFavorite } from '$lib/server/library/favorites';

export const load = async ({ locals, params }) => {
	const userId = requireWebUser(locals);
	const album = await item(db, Number(params.id));
	if (!album || album.kind !== 'album') throw error(404, 'Unbekanntes Album');
	const [trackRows, isFavorite] = await Promise.all([
		tracks(db, album.id),
		isItemFavorite(db, userId, album.id)
	]);
	return { album, tracks: trackRows, isFavorite };
};
