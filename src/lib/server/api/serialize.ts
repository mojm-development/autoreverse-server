import { items, tracks, chapters } from '../db/schema';

export type ItemRow = typeof items.$inferSelect;
export type TrackRow = typeof tracks.$inferSelect;
export type ChapterRow = typeof chapters.$inferSelect;

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
		missing_since: row.missingSince ?? null,
		published_at: row.publishedAt ?? null
	};
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
