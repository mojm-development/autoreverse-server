import { and, eq, isNotNull, sql } from 'drizzle-orm';
import { favorites, items as itemsTable, tracks as tracksTable } from '../db/schema';
import { naturalKey } from './sorting';
import type { DrizzleDb } from '../db';

export async function addItemFavorite(db: DrizzleDb, userId: number, itemId: number) {
	await db.execute(sql`
		INSERT INTO favorites (user_id, item_id) VALUES (${userId}, ${itemId})
		ON CONFLICT (user_id, item_id) WHERE item_id IS NOT NULL DO NOTHING
	`);
}
export async function removeItemFavorite(db: DrizzleDb, userId: number, itemId: number) {
	await db.delete(favorites).where(and(eq(favorites.userId, userId), eq(favorites.itemId, itemId)));
}
export async function addTrackFavorite(db: DrizzleDb, userId: number, trackId: number) {
	await db.execute(sql`
		INSERT INTO favorites (user_id, track_id) VALUES (${userId}, ${trackId})
		ON CONFLICT (user_id, track_id) WHERE track_id IS NOT NULL DO NOTHING
	`);
}
export async function removeTrackFavorite(db: DrizzleDb, userId: number, trackId: number) {
	await db
		.delete(favorites)
		.where(and(eq(favorites.userId, userId), eq(favorites.trackId, trackId)));
}
export async function listFavoriteItems(db: DrizzleDb, userId: number) {
	return db
		.select({ item: itemsTable })
		.from(favorites)
		.innerJoin(itemsTable, eq(itemsTable.id, favorites.itemId))
		.where(and(eq(favorites.userId, userId), isNotNull(favorites.itemId)))
		.orderBy(naturalKey(itemsTable.sortTitle))
		.then((rows) => rows.map((r) => r.item));
}
export async function listFavoriteTracks(db: DrizzleDb, userId: number) {
	return db
		.select({ track: tracksTable, item: itemsTable })
		.from(favorites)
		.innerJoin(tracksTable, eq(tracksTable.id, favorites.trackId))
		.innerJoin(itemsTable, eq(itemsTable.id, tracksTable.itemId))
		.where(and(eq(favorites.userId, userId), isNotNull(favorites.trackId)))
		.orderBy(naturalKey(itemsTable.sortTitle), tracksTable.position);
}

export async function isItemFavorite(
	db: DrizzleDb,
	userId: number,
	itemId: number
): Promise<boolean> {
	const rows = await db
		.select()
		.from(favorites)
		.where(and(eq(favorites.userId, userId), eq(favorites.itemId, itemId)));
	return rows.length > 0;
}
