import { fail } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { requireWebAdmin } from '$lib/server/auth/session';
import { loadConfig } from '$lib/server/config';
import { ApiError } from '$lib/server/api/errors';
import {
	albumsOf,
	chosenCover,
	fallbackCovers,
	selectAlbum,
	storeImage,
	clearCover
} from '$lib/server/library/artistCovers';

export const load = async ({ locals, params }) => {
	await requireWebAdmin(locals, db);
	const artist = params.name;
	const [albums, cover, fallback] = await Promise.all([
		albumsOf(db, artist),
		chosenCover(db, artist),
		fallbackCovers(db)
	]);
	const effectiveItemId =
		cover?.itemId ?? (cover?.imagePath ? null : (fallback.get(artist) ?? null));
	return {
		artist,
		albums: albums.map((a) => ({
			id: a.id,
			title: a.title,
			year: a.year,
			coverUrl: a.coverPath ? `/items/${a.id}/cover` : null
		})),
		hasCustomImage: Boolean(cover?.imagePath),
		customImageUrl: cover?.imagePath
			? `/artists/${encodeURIComponent(artist)}/image?v=${Date.now()}`
			: null,
		selectedItemId: cover?.itemId ?? null,
		effectiveItemId
	};
};

function message(e: unknown): string {
	return e instanceof ApiError ? e.detail : 'Das hat nicht geklappt';
}

export const actions = {
	select: async ({ locals, params, request }) => {
		await requireWebAdmin(locals, db);
		const form = await request.formData();
		const itemId = Number(form.get('item_id'));
		if (!Number.isInteger(itemId) || itemId <= 0) return fail(422, { error: 'Ungültige Album-ID' });
		try {
			await selectAlbum(db, params.name, itemId);
		} catch (e) {
			return fail(422, { error: message(e) });
		}
		return { ok: true };
	},
	upload: async ({ locals, params, request }) => {
		await requireWebAdmin(locals, db);
		const form = await request.formData();
		const file = form.get('image');
		if (!(file instanceof File) || file.size === 0) {
			return fail(422, { error: 'Keine Bilddatei ausgewählt' });
		}
		const config = loadConfig(process.env as Record<string, string | undefined>);
		try {
			await storeImage(
				db,
				params.name,
				{ type: file.type, size: file.size, bytes: new Uint8Array(await file.arrayBuffer()) },
				config.artistsDir
			);
		} catch (e) {
			return fail(422, { error: message(e) });
		}
		return { ok: true };
	},
	reset: async ({ locals, params }) => {
		await requireWebAdmin(locals, db);
		await clearCover(db, params.name);
		return { ok: true };
	}
};
