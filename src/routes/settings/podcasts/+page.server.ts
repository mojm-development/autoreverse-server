import { fail } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebAdmin } from '$lib/server/auth/session';
import { loadConfig } from '$lib/server/config';
import { getKeepDefault, setKeepDefault, KEEP_MAX } from '$lib/server/podcasts/retention';

export const load = async ({ locals }) => {
	await requireWebAdmin(locals, db);
	const config = loadConfig(process.env as Record<string, string | undefined>);
	return {
		keepDefault: await getKeepDefault(db),
		keepMax: KEEP_MAX,
		refreshHours: config.podcastRefreshHours
	};
};

export const actions = {
	keep: async ({ locals, request }) => {
		await requireWebAdmin(locals, db);
		const form = await request.formData();
		const keep = Number(form.get('keep'));
		if (!Number.isInteger(keep) || keep < 0 || keep > KEEP_MAX) {
			return fail(422, { error: `Bitte eine Zahl zwischen 0 und ${KEEP_MAX}` });
		}
		await setKeepDefault(db, keep);
		return { ok: true };
	}
};
