import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { artists } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	requireWebUser(locals);
	return { artists: await artists(db) };
};
