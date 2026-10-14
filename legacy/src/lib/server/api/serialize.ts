import { items, tracks, chapters } from '../db/schema';

export type ItemRow = typeof items.$inferSelect;
export type TrackRow = typeof tracks.$inferSelect;
export type ChapterRow = typeof chapters.$inferSelect;

export function toIso(d: Date | null): string | null {
	return d ? d.toISOString().replace(/\.\d{3}Z$/, 'Z') : null;
}

export function toItemSummary(row: ItemRow) {
	return {
		id: row.id,
		kind: row.kind,
		title: row.title,
		has_cover: Boolean(row.coverPath),
		author: row.author ?? null,
		artist: row.artist ?? null,
		series: row.series ?? null,
		year: row.year ?? null,
		missing_since: toIso(row.missingSince),
		published_at: toIso(row.publishedAt)
	};
}

/**
 * The description belongs to a single item, never to a list: blurbs run long, and a
 * library page would carry megabytes of prose for rows that show a title and a cover.
 * `toItemSummary` therefore leaves it out and this adds it back on top.
 */
export function toItemDetail(row: ItemRow) {
	return { ...toItemSummary(row), description: row.description ?? null };
}

export function toTrackOut(row: TrackRow) {
	return { id: row.id, position: row.position, title: row.title ?? null, duration: row.duration };
}

export function toChapterOut(row: ChapterRow) {
	return { title: row.title, start: row.start, end: row.end };
}

export function toTrackSummary(
	row: TrackRow & { item_id?: number; item_title?: string; item_kind?: string }
) {
	return {
		id: row.id,
		title: row.title ?? null,
		duration: row.duration,
		item_id: row.item_id,
		item_title: row.item_title,
		item_kind: row.item_kind
	};
}
