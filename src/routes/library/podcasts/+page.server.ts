import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { podcastOverview } from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const podcasts = await podcastOverview(db, userId);
	const unread = podcasts.reduce((sum, p) => sum + (p.unheard_count ?? 0), 0);
	return { podcasts, unread, query: url.searchParams.get('q') ?? '' };
};
