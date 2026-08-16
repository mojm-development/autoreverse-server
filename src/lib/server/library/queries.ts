import { and, eq, inArray, isNull, isNotNull, sql, desc } from 'drizzle-orm';
import {
	items as itemsTable,
	tracks as tracksTable,
	chapters as chaptersTable,
	favorites,
	progress as progressTable
} from '../db/schema';
import { likePattern } from './like';
import type { DrizzleDb } from '../db';

const SORTS: Record<string, ReturnType<typeof sql>> = {
	title: sql`lower(${itemsTable.sortTitle})`,
	added: sql`${itemsTable.addedAt} DESC, lower(${itemsTable.sortTitle})`
};
export const SORT_LABELS: Record<string, string> = { title: 'Titel A–Z', added: 'Zuletzt dazu' };

export interface ItemsFilter {
	kind?: string;
	q?: string;
	limit: number;
	offset: number;
	missing?: boolean;
	sort?: 'title' | 'added';
	favoritesOf?: number;
}

export async function items(db: DrizzleDb, filter: ItemsFilter) {
	const conditions = [isNull(itemsTable.parentId)];
	if (filter.kind) conditions.push(eq(itemsTable.kind, filter.kind));
	if (filter.q) {
		const pattern = likePattern(filter.q);
		conditions.push(
			sql`(${itemsTable.title} ILIKE ${pattern} ESCAPE '\\' OR ${itemsTable.author} ILIKE ${pattern} ESCAPE '\\' OR ${itemsTable.artist} ILIKE ${pattern} ESCAPE '\\')`
		);
	}
	if (filter.missing !== undefined) {
		conditions.push(
			filter.missing ? isNotNull(itemsTable.missingSince) : isNull(itemsTable.missingSince)
		);
	}
	let query = db.select({ item: itemsTable }).from(itemsTable).$dynamic();
	if (filter.favoritesOf !== undefined) {
		query = query.innerJoin(
			favorites,
			and(eq(favorites.itemId, itemsTable.id), eq(favorites.userId, filter.favoritesOf))
		);
	}
	const order = SORTS[filter.sort ?? 'title'] ?? SORTS.title;
	const rows = await query
		.where(and(...conditions))
		.orderBy(order)
		.limit(filter.limit)
		.offset(filter.offset);
	return rows.map((r) => r.item);
}

export async function item(db: DrizzleDb, id: number) {
	const [row] = await db.select().from(itemsTable).where(eq(itemsTable.id, id));
	return row ?? null;
}

export async function track(db: DrizzleDb, id: number) {
	const [row] = await db.select().from(tracksTable).where(eq(tracksTable.id, id));
	return row ?? null;
}

export async function tracks(db: DrizzleDb, itemId: number) {
	return db
		.select()
		.from(tracksTable)
		.where(eq(tracksTable.itemId, itemId))
		.orderBy(tracksTable.position);
}

export async function chapters(db: DrizzleDb, itemId: number) {
	return db
		.select()
		.from(chaptersTable)
		.where(eq(chaptersTable.itemId, itemId))
		.orderBy(chaptersTable.position);
}

export async function children(db: DrizzleDb, parentId: number) {
	return db
		.select()
		.from(itemsTable)
		.where(eq(itemsTable.parentId, parentId))
		.orderBy(sql`${itemsTable.publishedAt} DESC NULLS LAST`, itemsTable.id);
}

export async function countItems(db: DrizzleDb, kind: string): Promise<number> {
	const [{ n }] = await db
		.select({ n: sql<number>`count(*)::int` })
		.from(itemsTable)
		.where(and(isNull(itemsTable.parentId), eq(itemsTable.kind, kind)));
	return n;
}

export async function countMissing(db: DrizzleDb): Promise<number> {
	const [{ n }] = await db
		.select({ n: sql<number>`count(*)::int` })
		.from(itemsTable)
		.where(isNotNull(itemsTable.missingSince));
	return n;
}

export async function libraryCounts(db: DrizzleDb) {
	const rows = await db
		.select({ kind: itemsTable.kind, n: sql<number>`count(*)::int` })
		.from(itemsTable)
		.where(isNull(itemsTable.parentId))
		.groupBy(itemsTable.kind);
	const counts: Record<string, number> = { book: 0, album: 0, podcast: 0 };
	for (const row of rows) if (row.kind in counts) counts[row.kind] = row.n;
	const [{ n: trackCount }] = await db.select({ n: sql<number>`count(*)::int` }).from(tracksTable);
	return {
		book_count: counts.book,
		album_count: counts.album,
		podcast_count: counts.podcast,
		track_count: trackCount
	};
}

export async function deleteMissing(db: DrizzleDb): Promise<number> {
	const rows = await db
		.delete(itemsTable)
		.where(isNotNull(itemsTable.missingSince))
		.returning({ id: itemsTable.id });
	return rows.length;
}

export async function searchItems(db: DrizzleDb, q: string, kinds: string[], limit: number) {
	const pattern = likePattern(q);
	return db
		.select()
		.from(itemsTable)
		.where(
			and(
				inArray(itemsTable.kind, kinds),
				sql`(${itemsTable.title} ILIKE ${pattern} ESCAPE '\\' OR ${itemsTable.author} ILIKE ${pattern} ESCAPE '\\' OR ${itemsTable.artist} ILIKE ${pattern} ESCAPE '\\')`
			)
		)
		.orderBy(sql`lower(${itemsTable.sortTitle})`)
		.limit(limit);
}

export async function searchTracks(db: DrizzleDb, q: string, limit: number) {
	const pattern = likePattern(q);
	const rows = await db.execute(sql`
		SELECT track.id, track.title, track.duration, track.item_id,
		       item.title AS item_title, item.kind AS item_kind
		FROM tracks AS track JOIN items AS item ON item.id = track.item_id
		WHERE track.title ILIKE ${pattern} ESCAPE '\\'
		ORDER BY lower(track.title) LIMIT ${limit}
	`);
	return rows as unknown as Array<{
		id: number;
		title: string | null;
		duration: number;
		item_id: number;
		item_title: string;
		item_kind: string;
	}>;
}

export async function artists(db: DrizzleDb) {
	const rows = await db
		.select({ name: itemsTable.artist, albumCount: sql<number>`count(*)::int` })
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'album'), isNotNull(itemsTable.artist)))
		.groupBy(itemsTable.artist)
		.orderBy(sql`lower(${itemsTable.artist})`);
	return rows;
}

export async function albumsOfArtist(db: DrizzleDb, artist: string) {
	return db
		.select()
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'album'), eq(itemsTable.artist, artist)))
		.orderBy(sql`${itemsTable.year} DESC NULLS LAST`, sql`lower(${itemsTable.sortTitle})`);
}

export async function searchArtists(db: DrizzleDb, q: string, limit: number) {
	const pattern = likePattern(q);
	const rows = await db.execute(sql`
		SELECT name, role, count(*)::int AS work_count FROM (
			SELECT artist AS name, 'artist' AS role FROM items
			WHERE kind = 'album' AND artist IS NOT NULL AND artist ILIKE ${pattern} ESCAPE '\\'
			UNION ALL
			SELECT author AS name, 'author' AS role FROM items
			WHERE kind = 'book' AND author IS NOT NULL AND author ILIKE ${pattern} ESCAPE '\\'
		) AS named
		GROUP BY name, role ORDER BY lower(name) LIMIT ${limit}
	`);
	return rows as unknown as Array<{ name: string; role: 'artist' | 'author'; work_count: number }>;
}

export async function continueListening(db: DrizzleDb, userId: number, limit = 20) {
	const rows = await db.execute(sql`
		SELECT item.id, item.title, item.kind, item.author, item.artist,
		       item.series, item.series_index, item.cover_path, progress.position,
		       coalesce(total.duration, 0) AS duration
		FROM progress
		JOIN items AS item ON item.id = progress.item_id
		LEFT JOIN LATERAL (
			SELECT sum(tracks.duration) AS duration FROM tracks WHERE tracks.item_id = item.id
		) AS total ON true
		WHERE progress.user_id = ${userId} AND progress.finished = false
		ORDER BY progress.updated_at DESC LIMIT ${limit}
	`);
	return rows as unknown as Array<{
		id: number;
		title: string;
		kind: string;
		author: string | null;
		artist: string | null;
		series: string | null;
		series_index: number | null;
		cover_path: string | null;
		position: number;
		duration: number;
	}>;
}

export async function progress(db: DrizzleDb, userId: number, itemId: number) {
	const [row] = await db
		.select({ position: progressTable.position, finished: progressTable.finished })
		.from(progressTable)
		.where(and(eq(progressTable.userId, userId), eq(progressTable.itemId, itemId)));
	return row ?? null;
}

export async function progressMap(db: DrizzleDb, userId: number, itemIds: number[]) {
	if (itemIds.length === 0) return {};
	const rows = await db
		.select({
			item_id: progressTable.itemId,
			position: progressTable.position,
			finished: progressTable.finished,
			duration: sql<number>`coalesce(sum(${tracksTable.duration}), 0)`
		})
		.from(progressTable)
		.leftJoin(tracksTable, eq(tracksTable.itemId, progressTable.itemId))
		.where(and(eq(progressTable.userId, userId), inArray(progressTable.itemId, itemIds)))
		.groupBy(progressTable.itemId, progressTable.position, progressTable.finished);
	const map: Record<number, { position: number; finished: boolean; duration: number }> = {};
	for (const row of rows)
		map[row.item_id] = { position: row.position, finished: row.finished, duration: row.duration };
	return map;
}

export async function itemDurations(
	db: DrizzleDb,
	itemIds: number[]
): Promise<Record<number, number>> {
	if (itemIds.length === 0) return {};
	const rows = await db
		.select({
			itemId: tracksTable.itemId,
			duration: sql<number>`coalesce(sum(${tracksTable.duration}), 0)`
		})
		.from(tracksTable)
		.where(inArray(tracksTable.itemId, itemIds))
		.groupBy(tracksTable.itemId);
	const map: Record<number, number> = {};
	for (const row of rows) map[row.itemId] = row.duration;
	return map;
}

export async function seriesSiblings(db: DrizzleDb, series: string) {
	return db
		.select()
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'book'), eq(itemsTable.series, series)))
		.orderBy(sql`${itemsTable.seriesIndex} NULLS LAST`, sql`lower(${itemsTable.sortTitle})`);
}

export async function podcastOverview(db: DrizzleDb, userId: number) {
	const rows = await db.execute(sql`
		SELECT podcast.*,
		       count(episode.id)::int AS episode_count,
		       count(episode.id) FILTER (WHERE progress.item_id IS NULL OR progress.finished = false)::int AS unheard_count
		FROM items AS podcast
		LEFT JOIN items AS episode ON episode.parent_id = podcast.id
		LEFT JOIN progress ON progress.item_id = episode.id AND progress.user_id = ${userId}
		WHERE podcast.kind = 'podcast'
		GROUP BY podcast.id ORDER BY lower(podcast.sort_title)
	`);
	return rows as unknown as Array<{
		id: number;
		parent_id: number | null;
		kind: string;
		source_path: string | null;
		title: string;
		sort_title: string;
		cover_path: string | null;
		artist: string | null;
		album_artist: string | null;
		year: number | null;
		author: string | null;
		narrator: string | null;
		series: string | null;
		series_index: number | null;
		feed_url: string | null;
		last_checked: Date | null;
		published_at: Date | null;
		added_at: Date;
		missing_since: Date | null;
		guid: string | null;
		episode_count: number;
		unheard_count: number;
	}>;
}

export async function playlistOverview(db: DrizzleDb, userId: number) {
	const rows = await db.execute(sql`
		SELECT playlist.*,
		       count(playlist_entry.id)::int AS entry_count,
		       coalesce(sum(coalesce(single.duration, whole.duration, 0)), 0) AS duration
		FROM playlists AS playlist
		LEFT JOIN playlist_entries AS playlist_entry ON playlist_entry.playlist_id = playlist.id
		LEFT JOIN tracks AS single ON single.id = playlist_entry.track_id
		LEFT JOIN LATERAL (
			SELECT sum(tracks.duration) AS duration FROM tracks WHERE tracks.item_id = playlist_entry.item_id
		) AS whole ON true
		WHERE playlist.user_id = ${userId}
		GROUP BY playlist.id ORDER BY lower(playlist.name)
	`);
	return (
		rows as unknown as Array<{
			id: number;
			user_id: number;
			name: string;
			created_at: Date;
			entry_count: number;
			duration: number | string;
		}>
	).map((row) => {
		return {
			...row,
			entryCount: row.entry_count,
			duration: Number(row.duration)
		};
	});
}

export async function recentlyAdded(db: DrizzleDb, limit = 12) {
	return db
		.select()
		.from(itemsTable)
		.where(and(isNull(itemsTable.parentId), isNull(itemsTable.missingSince)))
		.orderBy(desc(itemsTable.addedAt), desc(itemsTable.id))
		.limit(limit);
}

export async function totalItems(db: DrizzleDb): Promise<number> {
	const [{ n }] = await db.select({ n: sql<number>`count(*)::int` }).from(itemsTable);
	return n;
}
