import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { authenticate, setPassword } from '$lib/server/auth/passwords';
import { revokeOtherTokens } from '$lib/server/auth/tokens';
import { users } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';
import { readJson } from '$lib/server/api/validate';

export async function _mePasswordPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'request'>
): Promise<Response> {
	try {
		const userId = requireApiUser(event.locals);
		const { current_password, new_password } = await readJson<{
			current_password: string;
			new_password: string;
		}>(event.request);
		if (typeof new_password !== 'string' || new_password.length < 8 || new_password.length > 200) {
			return apiError(422, 'new_password muss 8–200 Zeichen haben');
		}
		const [row] = await db.select({ name: users.name }).from(users).where(eq(users.id, userId));
		if (!row) return apiError(404, 'Unbekannter Nutzer');
		const verified = await authenticate(db, row.name, current_password);
		if (verified === null) return apiError(401, 'Aktuelles Passwort ist falsch');
		await setPassword(db, userId, new_password);
		if (event.locals.token) await revokeOtherTokens(db, userId, event.locals.token);
		return new Response(null, { status: 204 });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
}

export const POST: RequestHandler = (event) => _mePasswordPostHandler(defaultDb, event);
