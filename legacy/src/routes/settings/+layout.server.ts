import { redirect } from '@sveltejs/kit';

const ADMIN_ONLY = [
	'/settings/libraries',
	'/settings/scan',
	'/settings/users',
	'/settings/podcasts'
];

export const load = async ({ url, parent }) => {
	const parentData = await parent();
	if (!parentData.user) throw redirect(303, '/login');
	if (
		!parentData.user.isAdmin &&
		ADMIN_ONLY.some((p) => url.pathname === p || url.pathname.startsWith(`${p}/`))
	) {
		throw redirect(303, '/library');
	}
	return {};
};
