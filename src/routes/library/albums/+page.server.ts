import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	items,
	countItems,
	countMissing,
	itemDurations,
	albumsOfArtist,
	PAGE_SIZE,
	SORT_LABELS
} from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	requireWebUser(locals);
	const view: 'grid' | 'list' = url.searchParams.get('view') === 'list' ? 'list' : 'grid';
	const sort = (url.searchParams.get('sort') as 'title' | 'added') ?? 'title';
	const q = url.searchParams.get('q') ?? undefined;
	const artist = url.searchParams.get('artist') ?? undefined;
	const missing = url.searchParams.get('missing') === 'true';

	const [rows, total] = await Promise.all([
		artist
			? albumsOfArtist(db, artist)
			: items(db, { kind: 'album', sort, q, missing, limit: PAGE_SIZE, offset: 0 }),
		artist
			? albumsOfArtist(db, artist).then((r) => r.length)
			: missing
				? countMissing(db, 'album')
				: countItems(db, 'album', { q, missing })
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
		missing,
		albums: rows,
		total,
		durations,
		pageSize: PAGE_SIZE,
		hasMore: !artist && rows.length === PAGE_SIZE
	};
};
