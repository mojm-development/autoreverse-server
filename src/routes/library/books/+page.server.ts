import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { items, countItems, itemDurations, progressMap } from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const q = url.searchParams.get('q') ?? undefined;
	const [rows, total] = await Promise.all([
		items(db, { kind: 'book', sort: 'title', q, limit: 200, offset: 0 }),
		countItems(db, 'book')
	]);
	const ids = rows.map((r) => r.id);
	const [durations, progress] = await Promise.all([
		itemDurations(db, ids),
		progressMap(db, userId, ids)
	]);
	return { q: q ?? '', books: rows, total, durations, progress };
};
