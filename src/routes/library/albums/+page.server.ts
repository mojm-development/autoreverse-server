import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { items, countItems, itemDurations, SORT_LABELS } from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	requireWebUser(locals);
	const view: 'grid' | 'list' = url.searchParams.get('view') === 'list' ? 'list' : 'grid';
	const sort = (url.searchParams.get('sort') as 'title' | 'added') ?? 'title';
	const q = url.searchParams.get('q') ?? undefined;
	const [rows, total] = await Promise.all([
		items(db, { kind: 'album', sort, q, limit: 200, offset: 0 }),
		countItems(db, 'album')
	]);
	const durations = await itemDurations(
		db,
		rows.map((r) => r.id)
	);
	return { view, sort, sortLabel: SORT_LABELS[sort], q: q ?? '', albums: rows, total, durations };
};
