import { eq } from 'drizzle-orm';
import { chapters as chaptersTable } from '../db/schema';
import type { DrizzleDb } from '../db';

export interface ChapterMark {
	start: number;
	title: string;
}

/** No source gives us chapter ends — only where each one starts. */
const OPEN_END = 3600;

export function toChapterRows(
	marks: ChapterMark[],
	totalDuration: number
): { position: number; title: string; start: number; end: number }[] {
	const sorted = [...marks].filter((m) => m.start >= 0).sort((a, b) => a.start - b.start);
	return sorted.map((mark, index) => {
		const next = sorted[index + 1];
		// The last chapter runs to the end of the episode. Before the file is downloaded
		// its real length is unknown, so leave an hour rather than a zero-length chapter
		// the player can never be inside of.
		const end = next
			? next.start
			: totalDuration > mark.start
				? totalDuration
				: mark.start + OPEN_END;
		return {
			position: index + 1,
			title: mark.title || `Kapitel ${index + 1}`,
			start: mark.start,
			end
		};
	});
}

export async function saveEpisodeChapters(
	db: DrizzleDb,
	itemId: number,
	marks: ChapterMark[],
	totalDuration = 0
): Promise<number> {
	const rows = toChapterRows(marks, totalDuration);
	await db.delete(chaptersTable).where(eq(chaptersTable.itemId, itemId));
	if (rows.length === 0) return 0;
	await db.insert(chaptersTable).values(rows.map((row) => ({ ...row, itemId })));
	return rows.length;
}

/**
 * The Podcasting 2.0 chapter document: `{ "chapters": [{ "startTime": 0, "title": … }] }`.
 * Entries flagged `toc: false` are markers for the player, not chapters.
 */
export function parseChaptersJson(text: string): ChapterMark[] {
	let parsed: unknown;
	try {
		parsed = JSON.parse(text);
	} catch {
		return [];
	}
	const list = (parsed as { chapters?: unknown })?.chapters;
	if (!Array.isArray(list)) return [];
	const marks: ChapterMark[] = [];
	for (const entry of list) {
		if (!entry || typeof entry !== 'object') continue;
		const record = entry as { startTime?: unknown; title?: unknown; toc?: unknown };
		if (record.toc === false) continue;
		const start = Number(record.startTime);
		if (!Number.isFinite(start) || start < 0) continue;
		const title = typeof record.title === 'string' ? record.title.trim() : '';
		marks.push({ start, title });
	}
	return marks;
}

export async function fetchChaptersJson(url: string): Promise<ChapterMark[]> {
	try {
		const response = await fetch(url, {
			redirect: 'follow',
			signal: AbortSignal.timeout(10_000)
		});
		if (!response.ok) return [];
		return parseChaptersJson(await response.text());
	} catch {
		// Chapters are a bonus; a feed that cannot serve them must not fail a download.
		return [];
	}
}
