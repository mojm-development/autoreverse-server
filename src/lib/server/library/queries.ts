import { and, eq, isNull, isNotNull, sql } from 'drizzle-orm';
import {
	items as itemsTable,
	tracks as tracksTable,
	chapters as chaptersTable,
	favorites
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
