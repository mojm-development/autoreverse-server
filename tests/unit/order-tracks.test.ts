import { describe, it, expect } from 'vitest';
import { orderTracks, type RawTrack } from '../../src/lib/server/scanner/books';
import type { TrackTags } from '../../src/lib/server/scanner/tags';

function tags(overrides: Partial<TrackTags> = {}): TrackTags {
	return {
		title: null,
		artist: null,
		album: null,
		albumArtist: null,
		track: null,
		disc: null,
		year: null,
		series: null,
		seriesIndex: null,
		duration: 10,
		readable: true,
		...overrides
	};
}

function raw(path: string, t: Partial<TrackTags> = {}): RawTrack {
	return { path, tags: tags(t), mtime: 1, size: 2 };
}

/** Positions land in UNIQUE(item_id, position), so a repeated number is not a
 * cosmetic problem — the insert fails and takes the whole scan pass with it. */
describe('orderTracks', () => {
	it('numbers loose singles that each claim track 1 without colliding', () => {
		const files = ['/m/a.flac', '/m/b.flac', '/m/c.flac'];
		const tracks = orderTracks(
			files.map((f) => raw(f, { track: 1 })),
			files
		);
		expect(tracks.map((t) => t.position)).toEqual([1, 2, 3]);
	});

	it('numbers a two-disc album living in one folder across both discs', () => {
		const files = ['/m/d1t1.mp3', '/m/d1t2.mp3', '/m/d2t1.mp3', '/m/d2t2.mp3'];
		const tracks = orderTracks(
			[
				raw(files[2], { disc: 2, track: 1 }),
				raw(files[0], { disc: 1, track: 1 }),
				raw(files[3], { disc: 2, track: 2 }),
				raw(files[1], { disc: 1, track: 2 })
			],
			files
		);
		expect(tracks.map((t) => t.path)).toEqual([files[0], files[1], files[2], files[3]]);
		expect(tracks.map((t) => t.position)).toEqual([1, 2, 3, 4]);
	});

	it('numbers a folder mixing tagged and untagged files without colliding', () => {
		const files = ['/m/untagged.mp3', '/m/tagged.mp3'];
		const tracks = orderTracks([raw(files[0]), raw(files[1], { track: 1 })], files);
		expect(tracks.map((t) => t.position)).toEqual([1, 2]);
		expect(new Set(tracks.map((t) => t.position)).size).toBe(2);
	});

	it('keeps a normally tagged album in track order', () => {
		const files = ['/m/03.mp3', '/m/01.mp3', '/m/02.mp3'];
		const tracks = orderTracks(
			[raw(files[0], { track: 3 }), raw(files[1], { track: 1 }), raw(files[2], { track: 2 })],
			files
		);
		expect(tracks.map((t) => t.path)).toEqual(['/m/01.mp3', '/m/02.mp3', '/m/03.mp3']);
		expect(tracks.map((t) => t.position)).toEqual([1, 2, 3]);
	});

	it('closes gaps rather than preserving gappy track numbers', () => {
		const files = ['/m/a.mp3', '/m/b.mp3'];
		const tracks = orderTracks([raw(files[0], { track: 1 }), raw(files[1], { track: 7 })], files);
		expect(tracks.map((t) => t.position)).toEqual([1, 2]);
	});

	it('falls back to file order for ties, deterministically', () => {
		const files = ['/m/b.mp3', '/m/a.mp3'];
		const tracks = orderTracks([raw(files[1], { track: 5 }), raw(files[0], { track: 5 })], files);
		expect(tracks.map((t) => t.path)).toEqual(['/m/b.mp3', '/m/a.mp3']);
	});

	it('titles untagged files from their filename', () => {
		const tracks = orderTracks(
			[raw('/m/Gentleman - Hold a Medi Megamix.flac')],
			['/m/Gentleman - Hold a Medi Megamix.flac']
		);
		expect(tracks[0].title).toBe('Gentleman - Hold a Medi Megamix');
	});
});
