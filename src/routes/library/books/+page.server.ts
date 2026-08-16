import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	items,
	countItems,
	itemDurations,
	progressMap,
	seriesSiblings
} from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const q = url.searchParams.get('q') ?? undefined;
	const series = url.searchParams.get('series') ?? undefined;

	let rows, total;
	if (series) {
		rows = await seriesSiblings(db, series);
		total = rows.length;
	} else {
		[rows, total] = await Promise.all([
			items(db, { kind: 'book', sort: 'title', q, limit: 200, offset: 0 }),
			countItems(db, 'book')
		]);
	}
	const ids = rows.map((r) => r.id);
	const [durations, progress] = await Promise.all([
		itemDurations(db, ids),
		progressMap(db, userId, ids)
	]);
	return { q: q ?? '', series: series ?? null, books: rows, total, durations, progress };
};
