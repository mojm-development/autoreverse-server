import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { listUsers } from '$lib/server/auth/directory';
import { createUser } from '$lib/server/auth/passwords';
import { eq } from 'drizzle-orm';
import { users } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export async function _usersGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const rows = await listUsers(db);
		return json({
			users: rows.map((r) => ({
				id: r.id,
				name: r.name,
				is_admin: r.isAdmin,
				created_at: r.createdAt,
				last_seen_at: r.lastSeenAt
			}))
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _usersPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		const { name, password, is_admin = false } = await event.request.json();
		if (typeof name !== 'string' || name.length < 1 || name.length > 100)
			return apiError(422, 'name muss 1–100 Zeichen haben');
		if (typeof password !== 'string' || password.length < 8 || password.length > 200)
			return apiError(422, 'password muss 8–200 Zeichen haben');
		let id: number;
		try {
			id = await createUser(db, name, password, is_admin);
		} catch (e) {
			const err = e as { cause?: { code?: string } };
			if (err?.cause?.code === '23505') return apiError(409, 'Nutzername ist bereits vergeben');
			throw e;
		}
		const [row] = await db.select({ isAdmin: users.isAdmin }).from(users).where(eq(users.id, id));
		return json({ id, name, is_admin: row.isAdmin }, { status: 201 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _usersGetHandler(defaultDb, event);
export const POST: RequestHandler = (event) => _usersPostHandler(defaultDb, event);
