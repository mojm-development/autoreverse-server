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
	// Default is 'series', not 'title': an audiobook library is a set of
	// numbered instalments, and alphabetical order splits a series apart the
	// moment two of its folders carry differently-prefixed album tags.
	return (BOOK_SORTS as readonly string[]).includes(requested ?? '')
		? (requested as SortKey)
		: 'series';
}

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const q = url.searchParams.get('q') ?? undefined;
	const series = url.searchParams.get('series') ?? undefined;
	const sort = sortFrom(url);

	let rows, total;
	if (series) {
		rows = await seriesSiblings(db, series);
		total = rows.length;
	} else {
		[rows, total] = await Promise.all([
			items(db, { kind: 'book', sort, q, limit: 200, offset: 0 }),
			countItems(db, 'book')
		]);
	}
	const ids = rows.map((r) => r.id);
	const [durations, progress] = await Promise.all([
		itemDurations(db, ids),
		progressMap(db, userId, ids)
	]);
	return {
		q: q ?? '',
		series: series ?? null,
		books: rows,
		total,
		durations,
		progress,
		sort,
		// The series drill-down has its own fixed ordering (series_index, then
		// instalment number), so the selector would be a lie there.
		sortable: series === undefined,
		sortOptions: BOOK_SORTS.map((key) => ({ key, label: SORT_LABELS[key] }))
	};
};
