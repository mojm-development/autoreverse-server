import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	getPreferences,
	setPreferences,
	SPEED_MIN,
	SPEED_MAX,
	SKIP_MIN,
	SKIP_MAX
} from '$lib/server/auth/preferences';

export const load = async ({ locals }) => {
	const userId = requireWebUser(locals);
	return {
		prefs: await getPreferences(db, userId),
		bounds: { SPEED_MIN, SPEED_MAX, SKIP_MIN, SKIP_MAX }
	};
};

export const actions = {
	default: async ({ request, locals }) => {
		const userId = requireWebUser(locals);
		const data = await request.formData();
		const playbackSpeed = Number(data.get('playbackSpeed'));
		const skipBack = Number(data.get('skipBack'));
		const skipForward = Number(data.get('skipForward'));
		const stored = await setPreferences(db, userId, playbackSpeed, skipBack, skipForward);
		return { success: true, prefs: stored };
	}
};
