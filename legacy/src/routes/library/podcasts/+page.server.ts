import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { podcastOverview, newEpisodes } from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const showNew = url.searchParams.get('filter') === 'new';
	const [podcasts, episodes] = await Promise.all([
		podcastOverview(db, userId),
		showNew ? newEpisodes(db, userId) : Promise.resolve([])
	]);
	const unread = podcasts.reduce((sum, p) => sum + (p.unheard_count ?? 0), 0);
	return { podcasts, unread, showNew, episodes, query: url.searchParams.get('q') ?? '' };
};
