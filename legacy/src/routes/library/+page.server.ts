import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { continueListening, recentlyAdded } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	const userId = requireWebUser(locals);
	const [continueEntries, recent] = await Promise.all([
		continueListening(db, userId, 3),
		recentlyAdded(db, 6)
	]);
	return { continueEntries, recent };
};
