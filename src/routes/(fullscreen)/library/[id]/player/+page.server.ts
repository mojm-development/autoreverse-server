import { error } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { item, chapters, tracks, coverPathFor } from '$lib/server/library/queries';
import { bookmarksForItem } from '$lib/server/library/bookmarks';

export const load = async ({ locals, params }) => {
	const userId = requireWebUser(locals);
	const target = await item(db, Number(params.id));
	if (!target) throw error(404, 'Unbekanntes Item');
	const [chapterRows, bookmarkRows, trackRows, coverPath] = await Promise.all([
		chapters(db, target.id),
		bookmarksForItem(db, userId, target.id),
		tracks(db, target.id),
		coverPathFor(db, target)
	]);
	return {
		// An episode inherits the show's artwork; the item row itself has none.
		item: { ...target, coverPath },
		chapters: chapterRows,
		bookmarks: bookmarkRows,
		tracks: trackRows
	};
};
