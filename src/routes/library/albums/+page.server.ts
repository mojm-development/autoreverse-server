import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	items,
	countItems,
	itemDurations,
	albumsOfArtist,
	SORT_LABELS
} from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	requireWebUser(locals);
	const view: 'grid' | 'list' = url.searchParams.get('view') === 'list' ? 'list' : 'grid';
	const sort = (url.searchParams.get('sort') as 'title' | 'added') ?? 'title';
	const q = url.searchParams.get('q') ?? undefined;
	const artist = url.searchParams.get('artist') ?? undefined;

	const [rows, total] = await Promise.all([
		artist
			? albumsOfArtist(db, artist)
			: items(db, { kind: 'album', sort, q, limit: 200, offset: 0 }),
		artist ? albumsOfArtist(db, artist).then((r) => r.length) : countItems(db, 'album')
	]);
	const durations = await itemDurations(
		db,
		rows.map((r) => r.id)
	);
	return {
		view,
		sort,
		sortLabel: SORT_LABELS[sort],
		q: q ?? '',
		artist: artist ?? null,
		albums: rows,
		total,
		durations
	};
};
