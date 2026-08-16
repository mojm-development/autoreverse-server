import { eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { users } from '$lib/server/db/schema';
import { libraryCounts, artists, podcastOverview } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	if (locals.userId === null) return { user: null };
	const [user] = await db
		.select({ id: users.id, name: users.name, isAdmin: users.isAdmin })
		.from(users)
		.where(eq(users.id, locals.userId));
	const [counts, artistRows, podcasts] = await Promise.all([
		libraryCounts(db),
		artists(db),
		podcastOverview(db, locals.userId)
	]);
	const unreadEpisodes = podcasts.reduce(
		(sum: number, p: { unheard_count?: number }) => sum + (p.unheard_count ?? 0),
		0
	);
	return {
		user: user ? { name: user.name, isAdmin: user.isAdmin } : null,
		counts: {
			albums: counts.album_count,
			artists: artistRows.length,
			podcasts: counts.podcast_count,
			unreadEpisodes,
			books: counts.book_count
		}
	};
};
