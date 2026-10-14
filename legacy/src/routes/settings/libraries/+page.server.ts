import { db } from '$lib/server/db';
import { requireWebUser, requireWebAdmin } from '$lib/server/auth/session';
import { getLibraryPaths, setLibraryPaths } from '$lib/server/settings/libraryPaths';
import { countMissing, deleteMissing } from '$lib/server/library/queries';

export const load = async ({ locals }) => {
	requireWebUser(locals);
	const [paths, missing] = await Promise.all([getLibraryPaths(db), countMissing(db)]);
	return { paths, missing };
};

export const actions = {
	savePaths: async ({ request, locals }) => {
		await requireWebAdmin(locals, db);
		const data = await request.formData();
		const booksDir = String(data.get('booksDir') ?? '').trim();
		const musicDir = String(data.get('musicDir') ?? '').trim();
		if (!booksDir || !musicDir) return { error: 'Beide Pfade werden benötigt.' };
		await setLibraryPaths(db, { booksDir, musicDir });
		return { success: true };
	},
	cleanupMissing: async ({ locals }) => {
		await requireWebAdmin(locals, db);
		const removed = await deleteMissing(db);
		return { success: true, removed };
	}
};
