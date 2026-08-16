import { error } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	item,
	children,
	podcastOverview,
	itemDurations,
	progressMap
} from '$lib/server/library/queries';

export const load = async ({ locals, params }) => {
	const userId = requireWebUser(locals);
	const podcast = await item(db, Number(params.id));
	if (!podcast || podcast.kind !== 'podcast') throw error(404, 'Unbekannter Podcast');
	const [podcasts, episodes] = await Promise.all([
		podcastOverview(db, userId),
		children(db, podcast.id)
	]);
	const episodeIds = episodes.map((e) => e.id);
	const [durations, progress] = await Promise.all([
		itemDurations(db, episodeIds),
		progressMap(db, userId, episodeIds)
	]);
	const unread = podcasts.reduce((sum, p) => sum + (p.unheard_count ?? 0), 0);
	return { podcast, podcasts, unread, episodes, durations, progress };
};
