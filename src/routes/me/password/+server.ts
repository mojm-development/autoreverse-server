import type { RequestHandler } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { authenticate, setPassword } from '$lib/server/auth/passwords';
import { revokeOtherTokens } from '$lib/server/auth/tokens';
import { users } from '$lib/server/db/schema';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export const POST: RequestHandler = async ({ locals, request }) => {
	try {
		const userId = requireApiUser(locals);
		const { current_password, new_password } = await request.json();
		if (typeof new_password !== 'string' || new_password.length < 8 || new_password.length > 200) {
			return apiError(422, 'new_password muss 8–200 Zeichen haben');
		}
		const [row] = await db.select({ name: users.name }).from(users).where(eq(users.id, userId));
		if (!row) return apiError(404, 'Unbekannter Nutzer');
		const verified = await authenticate(db, row.name, current_password);
		if (verified === null) return apiError(401, 'Aktuelles Passwort ist falsch');
		await setPassword(db, userId, new_password);
		if (locals.token) await revokeOtherTokens(db, userId, locals.token);
		return new Response(null, { status: 204 });
	} catch (err) {
		if (err instanceof ApiError) {
			return apiError(err.status, err.detail, err.retryAfter);
		}
		throw err;
	}
};
