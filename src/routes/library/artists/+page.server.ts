import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { artists } from '$lib/server/library/queries';
import { chosenCovers, fallbackCovers } from '$lib/server/library/artistCovers';

export const load = async ({ locals }) => {
	requireWebUser(locals);
	const [list, chosen, fallback] = await Promise.all([
		artists(db),
		chosenCovers(db),
		fallbackCovers(db)
	]);
	return {
		artists: list.map((entry) => {
			const name = entry.name ?? '';
			const pick = chosen.get(name);
			if (pick?.imagePath) {
				return {
					...entry,
					name,
					coverUrl: `/artists/${encodeURIComponent(name)}/image?v=${Date.now()}`
				};
			}
			const itemId = pick?.itemId ?? fallback.get(name) ?? null;
			return { ...entry, name, coverUrl: itemId ? `/items/${itemId}/cover` : null };
		})
	};
};
