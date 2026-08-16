import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { users } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { authenticate, setPassword } from '$lib/server/auth/passwords';
import { revokeOtherTokens } from '$lib/server/auth/tokens';

export const actions = {
	default: async ({ request, locals }) => {
		const userId = requireWebUser(locals);
		const data = await request.formData();
		const currentPassword = String(data.get('currentPassword') ?? '');
		const newPassword = String(data.get('newPassword') ?? '');
		if (newPassword.length < 8) return { error: 'Neues Passwort muss mindestens 8 Zeichen haben.' };
		const [row] = await db.select({ name: users.name }).from(users).where(eq(users.id, userId));
		if (!row) return { error: 'Unbekannter Nutzer.' };
		const verified = await authenticate(db, row.name, currentPassword);
		if (verified === null) return { error: 'Aktuelles Passwort ist falsch.' };
		await setPassword(db, userId, newPassword);
		if (locals.token) await revokeOtherTokens(db, userId, locals.token);
		return { success: true };
	}
};
