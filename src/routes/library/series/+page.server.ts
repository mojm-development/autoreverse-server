import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { bookSeries } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	const userId = requireWebUser(locals);
	return { series: await bookSeries(db, userId) };
};
