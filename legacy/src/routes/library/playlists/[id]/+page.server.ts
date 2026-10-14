import { error } from '@sveltejs/kit';
import { eq, and } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { playlists as playlistsTable } from '$lib/server/db/schema';
import { listEntries } from '$lib/server/library/playlistEntries';
import { item, track } from '$lib/server/library/queries';

export const load = async ({ locals, params }) => {
	const userId = requireWebUser(locals);
	const [playlist] = await db
		.select()
		.from(playlistsTable)
		.where(and(eq(playlistsTable.id, Number(params.id)), eq(playlistsTable.userId, userId)));
	if (!playlist) throw error(404, 'Unbekannte Playlist');

	const entries = await listEntries(db, playlist.id);
	const resolved = await Promise.all(
		entries.map(async (e) => {
			if (e.itemId) {
				const row = await item(db, e.itemId);
				return {
					id: e.id,
					position: e.position,
					title: row?.title ?? '',
					subtitle: row?.artist ?? row?.author ?? '',
					duration: null as number | null
				};
			}
			const t = await track(db, e.trackId!);
			if (!t) return { id: e.id, position: e.position, title: '', subtitle: '', duration: null };
			const parent = await item(db, t.itemId);
			return {
				id: e.id,
				position: e.position,
				title: t.title ?? parent?.title ?? '',
				subtitle: parent?.title ?? '',
				duration: t.duration
			};
		})
	);
	return { playlist, entries: resolved };
};
