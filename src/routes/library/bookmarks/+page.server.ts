import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { allBookmarks } from '$lib/server/library/bookmarks';

export const load = async ({ locals }) => {
	const userId = requireWebUser(locals);
	return { bookmarks: await allBookmarks(db, userId) };
};
