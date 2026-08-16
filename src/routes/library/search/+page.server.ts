import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import {
	searchItems,
	searchTracks,
	searchArtists,
	totalItems,
	progressMap
} from '$lib/server/library/queries';

export const load = async ({ locals, url }) => {
	const userId = requireWebUser(locals);
	const q = url.searchParams.get('q') ?? '';
	if (!q) {
		return {
			q,
			books: [],
			albums: [],
			podcasts: [],
			tracks: [],
			artists: [],
			total: await totalItems(db),
			elapsedMs: 0,
			progress: {}
		};
	}
	const started = performance.now();
	const [books, albums, podcasts, tracks, artistResults, total] = await Promise.all([
		searchItems(db, q, ['book'], 20),
		searchItems(db, q, ['album'], 20),
		searchItems(db, q, ['podcast', 'episode'], 20),
		searchTracks(db, q, 20),
		searchArtists(db, q, 20),
		totalItems(db)
	]);
	const progress = await progressMap(
		db,
		userId,
		[...books, ...albums, ...podcasts].map((r) => r.id)
	);
	return {
		q,
		books,
		albums,
		podcasts,
		tracks,
		artists: artistResults,
		total,
		elapsedMs: Math.round(performance.now() - started),
		progress
	};
};
