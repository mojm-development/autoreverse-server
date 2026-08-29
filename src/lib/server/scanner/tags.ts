import { parseFile, type IAudioMetadata } from 'music-metadata';

export interface TrackTags {
	title: string | null;
	artist: string | null;
	album: string | null;
	albumArtist: string | null;
	track: number | null;
	disc: number | null;
	year: number | null;
	/** Series name from the tags — MVNM/grouping, or a TXXX:SERIES frame. */
	series: string | null;
	/** Volume number from MVIN or TXXX:SERIES-PART. */
	seriesIndex: number | null;
	duration: number;
	readable: boolean;
}

const UNREADABLE: TrackTags = {
	title: null,
	artist: null,
	album: null,
	albumArtist: null,
	track: null,
	disc: null,
	year: null,
	series: null,
	seriesIndex: null,
	duration: 0,
	readable: false
};

/**
 * Series live in different places depending on who wrote the file: ID3 movement
 * frames (MVNM/MVIN, what Plex and Audiobookshelf write), the grouping frame, or
 * a free-form TXXX:SERIES. Read all of them, first hit wins.
 */
const SERIES_KEYS = ['series', 'book series', 'series-name', 'mvnm', 'show'];
const SERIES_PART_KEYS = ['series-part', 'series_index', 'part', 'mvin', 'movement'];

function nativeTag(meta: IAudioMetadata, wanted: string[]): string | null {
	for (const frames of Object.values(meta.native ?? {})) {
		for (const frame of frames) {
			const id = frame.id.toLowerCase();
			// TXXX:SERIES and ----:com.apple.iTunes:SERIES both end in the description.
			const description = id.split(':').pop() ?? id;
			if (!wanted.includes(description)) continue;
			const value = frame.value;
			const text =
				typeof value === 'string'
					? value
					: typeof value === 'object' && value !== null && 'text' in value
						? String((value as { text: unknown }).text)
						: null;
			if (text && text.trim()) return text.trim();
		}
	}
	return null;
}

function toNumber(value: string | null): number | null {
	if (!value) return null;
	// "3", "3/12", "3.5"
	const match = value.match(/^(\d{1,4}(?:[.,]\d{1,2})?)/);
	if (!match) return null;
	const parsed = Number(match[1].replace(',', '.'));
	return Number.isFinite(parsed) ? parsed : null;
}

export async function readTags(path: string): Promise<TrackTags> {
	try {
		const meta = await parseFile(path, { duration: true });
		const common = meta.common;
		const duration = meta.format.duration ?? 0;

		if (!meta.format.codec || duration === 0) {
			return { ...UNREADABLE };
		}

		return {
			title: common.title ?? null,
			artist: common.artist ?? null,
			album: common.album ?? null,
			albumArtist: common.albumartist ?? null,
			track: common.track?.no ?? null,
			disc: common.disk?.no ?? null,
			year: common.year ?? null,
			series: common.movement ?? nativeTag(meta, SERIES_KEYS) ?? (common.grouping || null) ?? null,
			seriesIndex: common.movementIndex?.no ?? toNumber(nativeTag(meta, SERIES_PART_KEYS)) ?? null,
			duration,
			readable: true
		};
	} catch {
		return { ...UNREADABLE };
	}
}
