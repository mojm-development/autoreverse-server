import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { listFavoriteItems, listFavoriteTracks } from '$lib/server/library/favorites';

export const load = async ({ locals }) => {
	const userId = requireWebUser(locals);
	const [items, tracks] = await Promise.all([
		listFavoriteItems(db, userId),
		listFavoriteTracks(db, userId)
	]);
	return { items, tracks };
};
