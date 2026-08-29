import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	items,
	countItems,
	itemDurations,
	progressMap,
	seriesSiblings,
	BOOK_SORTS,
	SORT_LABELS,
	type SortKey
} from '$lib/server/library/queries';

function sortFrom(url: URL): SortKey {
	const requested = url.searchParams.get('sort');
	return (BOOK_SORTS as readonly string[]).includes(requested ?? '')
		? (requested as SortKey)
		: 'series';
}

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const q = url.searchParams.get('q') ?? undefined;
	const series = url.searchParams.get('series') ?? undefined;
	const sort = sortFrom(url);
	const view: 'grid' | 'list' = url.searchParams.get('view') === 'list' ? 'list' : 'grid';

	let rows, total;
	if (series) {
		rows = await seriesSiblings(db, series);
		total = rows.length;
	} else {
		[rows, total] = await Promise.all([
			items(db, { kind: 'book', sort, q, missing: false, limit: 200, offset: 0 }),
			countItems(db, 'book')
		]);
	}
	const ids = rows.map((r) => r.id);
	const [durations, progress] = await Promise.all([
		itemDurations(db, ids),
		progressMap(db, userId, ids)
	]);
	return {
		view,
		q: q ?? '',
		series: series ?? null,
		books: rows,
		total,
		durations,
		progress,
		sort,
		sortable: series === undefined,
		sortOptions: BOOK_SORTS.map((key) => ({ key, label: SORT_LABELS[key] }))
	};
};
