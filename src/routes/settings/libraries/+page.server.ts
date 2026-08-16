import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { getLibraryPaths, setLibraryPaths } from '$lib/server/settings/libraryPaths';
import { listUsers, setAdmin } from '$lib/server/auth/directory';
import { createUser } from '$lib/server/auth/passwords';
import { countMissing, deleteMissing } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	requireWebUser(locals);
	const [paths, users, missing] = await Promise.all([
		getLibraryPaths(db),
		listUsers(db),
		countMissing(db)
	]);
	return { paths, users, missing };
};

export const actions = {
	savePaths: async ({ request }) => {
		const data = await request.formData();
		const booksDir = String(data.get('booksDir') ?? '').trim();
		const musicDir = String(data.get('musicDir') ?? '').trim();
		if (!booksDir || !musicDir) return { error: 'Beide Pfade werden benötigt.' };
		await setLibraryPaths(db, { booksDir, musicDir });
		return { success: true };
	},
	cleanupMissing: async () => {
		const removed = await deleteMissing(db);
		return { success: true, removed };
	},
	createUser: async ({ request }) => {
		const data = await request.formData();
		const name = String(data.get('name') ?? '');
		const password = String(data.get('password') ?? '');
		if (name.length < 1 || password.length < 8)
			return { error: 'Name und Passwort (min. 8 Zeichen) erforderlich.' };
		await createUser(db, name, password, false);
		return { success: true };
	},
	toggleAdmin: async ({ request }) => {
		const data = await request.formData();
		const userId = Number(data.get('userId'));
		const isAdmin = data.get('isAdmin') === 'true';
		await setAdmin(db, userId, isAdmin);
		return { success: true };
	}
};
