import { items, tracks, chapters } from '../db/schema';

export type ItemRow = typeof items.$inferSelect;
export type TrackRow = typeof tracks.$inferSelect;
export type ChapterRow = typeof chapters.$inferSelect;

/**
 * Drizzle's `timestamp({ withTimezone: true })` reaches `json()` as a JS `Date`,
 * which serializes via `Date.prototype.toJSON` with a `.000` fractional-seconds
 * component that pydantic's `datetime` never emits (I-9). Strip it so the wire
 * format matches the ground truth byte-for-byte — some strict ISO-8601 parsers
 * (e.g. Swift's `ISO8601DateFormatter()` without `.withFractionalSeconds`) reject
 * the fractional form.
 */
export function toIso(d: Date | null): string | null {
	return d ? d.toISOString().replace(/\.\d{3}Z$/, 'Z') : null;
}

export function toItemSummary(row: ItemRow) {
	return {
		id: row.id,
		kind: row.kind,
		title: row.title,
		// The ground truth's ItemDetail/ItemSummary Pydantic models bypass their own
		// `from_row` factory and always return `has_cover: false` here (a Python-side
		// bug, see final-branch-review.md M-12) — this port intentionally returns the
		// real value instead, a deliberate divergence, not a mystery.
		has_cover: Boolean(row.coverPath),
		author: row.author ?? null,
		artist: row.artist ?? null,
		series: row.series ?? null,
		year: row.year ?? null,
		missing_since: toIso(row.missingSince),
		published_at: toIso(row.publishedAt)
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
