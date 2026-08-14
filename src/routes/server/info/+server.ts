import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { eq, sql } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { libraryCounts } from '$lib/server/library/queries';
import { users } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function serverInfoGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const counts = await libraryCounts(db);
		const [caller] = await db
			.select({ isAdmin: users.isAdmin })
			.from(users)
			.where(eq(users.id, userId));
		let userCount: number | null = null;
		if (caller?.isAdmin) {
			const [{ n }] = await db.select({ n: sql<number>`count(*)::int` }).from(users);
			userCount = n;
		}
		return json({ ...counts, user_count: userCount });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => serverInfoGetHandler(defaultDb, event);
