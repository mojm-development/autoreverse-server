import { parseFile } from 'music-metadata';

export interface TrackTags {
	title: string | null;
	artist: string | null;
	album: string | null;
	albumArtist: string | null;
	track: number | null;
	disc: number | null;
	year: number | null;
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
	duration: 0,
	readable: false
};

/** Direct port of scanner/tags.py::read_tags — music-metadata's `common`
 * block plays the same role as mutagen's `easy=True` unified tag interface. */
export async function readTags(path: string): Promise<TrackTags> {
	try {
		const meta = await parseFile(path, { duration: true });
		const common = meta.common;
		const duration = meta.format.duration ?? 0;

		// Validate that we have actual audio metadata
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
			duration,
			readable: true
		};
	} catch {
		return { ...UNREADABLE };
	}
}
