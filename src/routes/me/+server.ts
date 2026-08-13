import { json, type RequestHandler } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { users } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export const GET: RequestHandler = async ({ locals }) => {
	try {
		const userId = requireApiUser(locals);
		const [row] = await db
			.select({ id: users.id, name: users.name, isAdmin: users.isAdmin })
			.from(users)
			.where(eq(users.id, userId));
		if (!row) return apiError(404, 'Unbekannter Nutzer');
		return json({ id: row.id, name: row.name, is_admin: row.isAdmin });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
};
