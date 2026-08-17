import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { podcastOverview } from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const podcasts = await podcastOverview(db, userId);
	const unread = podcasts.reduce((sum, p) => sum + (p.unheard_count ?? 0), 0);
	// The directory search lives in the URL so returning from a preview (or a
	// reload, or a shared link) lands back on the same result list.
	return { podcasts, unread, query: url.searchParams.get('q') ?? '' };
};
