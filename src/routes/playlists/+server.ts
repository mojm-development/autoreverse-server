import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { eq, sql } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { playlists, playlistEntries } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

function serialize(p: { id: number; name: string; createdAt: Date; entryCount: number }) {
	return { id: p.id, name: p.name, created_at: p.createdAt, entry_count: p.entryCount };
}

export async function _playlistsGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const rows = await db
			.select({
				id: playlists.id,
				name: playlists.name,
				createdAt: playlists.createdAt,
				entryCount: sql<number>`count(${playlistEntries.id})::int`
			})
			.from(playlists)
			.leftJoin(playlistEntries, eq(playlistEntries.playlistId, playlists.id))
			.where(eq(playlists.userId, userId))
			.groupBy(playlists.id)
			.orderBy(sql`lower(${playlists.name})`);
		return json({ playlists: rows.map(serialize) });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export async function _playlistsPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { name } = await event.request.json();
		const [row] = await db.insert(playlists).values({ userId, name }).returning();
		return json(serialize({ ...row, entryCount: 0 }));
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}
export const GET: RequestHandler = (event) => _playlistsGetHandler(defaultDb, event);
export const POST: RequestHandler = (event) => _playlistsPostHandler(defaultDb, event);
