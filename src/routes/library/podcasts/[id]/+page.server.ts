import { error, fail } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	item,
	children,
	podcastOverview,
	itemDurations,
	progressMap
} from '$lib/server/library/queries';
import { requireWebAdmin } from '$lib/server/auth/session';
import { getKeepDefault, setKeepForPodcast, KEEP_MAX } from '$lib/server/podcasts/retention';

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
	return {
		podcast,
		podcasts,
		unread,
		episodes,
		durations,
		progress,
		keepDefault: await getKeepDefault(db),
		keepMax: KEEP_MAX
	};
};

export const actions = {
	keep: async ({ locals, params, request }) => {
		await requireWebAdmin(locals, db);
		const form = await request.formData();
		const raw = String(form.get('keep') ?? '');
		if (raw === '') {
			await setKeepForPodcast(db, Number(params.id), null);
			return { ok: true };
		}
		const keep = Number(raw);
		if (!Number.isInteger(keep) || keep < 0 || keep > KEEP_MAX) {
			return fail(422, { error: `Bitte eine Zahl zwischen 0 und ${KEEP_MAX}` });
		}
		await setKeepForPodcast(db, Number(params.id), keep);
		return { ok: true };
	}
};
