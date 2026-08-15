import { eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { users } from '$lib/server/db/schema';
import { libraryCounts } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	if (locals.userId === null) return { user: null };
	const [user] = await db
		.select({ id: users.id, name: users.name, isAdmin: users.isAdmin })
		.from(users)
		.where(eq(users.id, locals.userId));
	const counts = await libraryCounts(db);
	const artistCount = 0; // wired properly to artists() count in the Start screen task (Task 33) — placeholder here keeps the shell generic
	return {
		user: user ? { name: user.name, isAdmin: user.isAdmin } : null,
		counts: {
			albums: counts.album_count,
			artists: artistCount,
			podcasts: counts.podcast_count,
			unreadEpisodes: 0, // wired in Task 37 (Podcasts screen) via podcastOverview
			books: counts.book_count
		}
	};
};
