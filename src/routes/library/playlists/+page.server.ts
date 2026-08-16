import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { playlistOverview } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	const userId = requireWebUser(locals);
	const playlists = await playlistOverview(db, userId);
	return { playlists };
};
