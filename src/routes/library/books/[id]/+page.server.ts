import { error } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { item, tracks, chapters, progress, seriesSiblings } from '$lib/server/library/queries';
import { bookmarksForItem } from '$lib/server/library/bookmarks';

export const load = async ({ locals, params }) => {
	const userId = requireWebUser(locals);
	const book = await item(db, Number(params.id));
	if (!book || book.kind !== 'book') throw error(404, 'Unbekanntes Hörbuch');
	const [trackRows, chapterRows, progressRow, bookmarkRows, seriesBooks] = await Promise.all([
		tracks(db, book.id),
		chapters(db, book.id),
		progress(db, userId, book.id),
		bookmarksForItem(db, userId, book.id),
		book.series ? seriesSiblings(db, book.series) : Promise.resolve([])
	]);
	return {
		book,
		tracks: trackRows,
		chapters: chapterRows,
		progress: progressRow,
		bookmarks: bookmarkRows,
		seriesBooks
	};
};
