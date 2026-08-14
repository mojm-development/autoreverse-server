import { json, type RequestHandler } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { setAdmin, LastAdminError } from '$lib/server/auth/directory';
import { users } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export const PATCH: RequestHandler = async ({ locals, params, request, platform }) => {
	try {
		const db = (platform as unknown as { context: { db: DrizzleDb } })?.context?.db ?? defaultDb;
		await requireApiAdmin(locals, db);
		const { is_admin } = await request.json();
		const userId = Number(params.id);
		try {
			const resultingIsAdmin = await setAdmin(db, userId, is_admin);
			const [row] = await db.select({ name: users.name }).from(users).where(eq(users.id, userId));
			return json({ id: userId, name: row.name, is_admin: resultingIsAdmin });
		} catch (e) {
			if (e instanceof LastAdminError)
				return apiError(409, 'Der letzte Verwalter kann nicht herabgestuft werden');
			return apiError(404, 'Unbekannter Nutzer');
		}
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
};
