import { db } from '$lib/server/db';
import { requireWebUser, requireWebAdmin } from '$lib/server/auth/session';
import { listUsers, setAdmin } from '$lib/server/auth/directory';
import { createUser } from '$lib/server/auth/passwords';

export const load = async ({ locals }) => {
	requireWebUser(locals);
	return { users: await listUsers(db) };
};

export const actions = {
	createUser: async ({ request, locals }) => {
		await requireWebAdmin(locals, db);
		const data = await request.formData();
		const name = String(data.get('name') ?? '');
		const password = String(data.get('password') ?? '');
		if (name.length < 1 || password.length < 8)
			return { error: 'Name und Passwort (min. 8 Zeichen) erforderlich.' };
		await createUser(db, name, password, false);
		return { success: true };
	},
	toggleAdmin: async ({ request, locals }) => {
		await requireWebAdmin(locals, db);
		const data = await request.formData();
		await setAdmin(db, Number(data.get('userId')), data.get('isAdmin') === 'true');
		return { success: true };
	}
};
