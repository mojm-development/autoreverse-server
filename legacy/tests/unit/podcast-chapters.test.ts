import { describe, it, expect } from 'vitest';
import { toChapterRows, parseChaptersJson } from '../../src/lib/server/podcasts/chapters';
import { parseFeed, pscTimeToSeconds } from '../../src/lib/server/podcasts/feed';

describe('pscTimeToSeconds', () => {
	it('reads Podlove normal play time in all its shapes', () => {
		expect(pscTimeToSeconds('00:00:00.000')).toBe(0);
		expect(pscTimeToSeconds('00:12:30')).toBe(750);
		expect(pscTimeToSeconds('1:02:00.500')).toBe(3720.5);
		expect(pscTimeToSeconds('45')).toBe(45);
	});

	it('refuses anything that is not a timestamp', () => {
		expect(pscTimeToSeconds('später')).toBeNull();
		expect(pscTimeToSeconds('')).toBeNull();
		expect(pscTimeToSeconds(undefined)).toBeNull();
	});
});

const FEED = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:psc="http://podlove.org/simple-chapters"
     xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"
     xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Apfelfunk</title>
    <item>
      <title>Folge 500</title><guid>abc</guid>
      <enclosure url="https://x/500.mp3" type="audio/mpeg" length="1"/>
      <itunes:duration>01:00:00</itunes:duration>
      <psc:chapters version="1.2">
        <psc:chapter start="00:12:30" title="Thema 1"/>
        <psc:chapter start="00:00:00.000" title="Begrüßung"/>
        <psc:chapter start="00:12:30" title="Doppelt"/>
        <psc:chapter start="kaputt" title="Ungültig"/>
      </psc:chapters>
    </item>
    <item>
      <title>Folge 499</title><guid>def</guid>
      <enclosure url="https://x/499.mp3" type="audio/mpeg" length="1"/>
      <podcast:chapters url="https://x/499.json" type="application/json+chapters"/>
    </item>
    <item><title>Folge 498</title><guid>ghi</guid></item>
  </channel>
</rss>`;

describe('parseFeed chapters', () => {
	it('takes Podlove chapters out of the feed, sorted and deduplicated', async () => {
		const feed = await parseFeed(FEED);
		expect(feed.episodes[0].chapters).toEqual([
			{ start: 0, title: 'Begrüßung' },
			{ start: 750, title: 'Thema 1' }
		]);
		expect(feed.episodes[0].chaptersUrl).toBeNull();
	});

	it('keeps the Podcasting 2.0 chapter URL for download time', async () => {
		const feed = await parseFeed(FEED);
		expect(feed.episodes[1].chaptersUrl).toBe('https://x/499.json');
		expect(feed.episodes[1].chapters).toEqual([]);
	});

	it('leaves an episode without either alone', async () => {
		const feed = await parseFeed(FEED);
		expect(feed.episodes[2].chapters).toEqual([]);
		expect(feed.episodes[2].chaptersUrl).toBeNull();
	});
});

describe('toChapterRows', () => {
	it('closes each chapter where the next one starts, the last one at the episode end', () => {
		expect(
			toChapterRows(
				[
					{ start: 750, title: 'B' },
					{ start: 0, title: 'A' }
				],
				1800
			)
		).toEqual([
			{ position: 1, title: 'A', start: 0, end: 750 },
			{ position: 2, title: 'B', start: 750, end: 1800 }
		]);
	});

	it('leaves the last chapter open-ended while the length is still unknown', () => {
		const rows = toChapterRows([{ start: 0, title: 'A' }], 0);
		// Zero-length would be a chapter the player can never be inside of.
		expect(rows[0].end).toBeGreaterThan(rows[0].start);
	});

	it('names an untitled chapter rather than storing an empty string', () => {
		expect(toChapterRows([{ start: 0, title: '' }], 10)[0].title).toBe('Kapitel 1');
	});
});

describe('parseChaptersJson', () => {
	it('reads the Podcasting 2.0 document and skips non-toc markers', () => {
		const json = JSON.stringify({
			version: '1.2.0',
			chapters: [
				{ startTime: 0, title: 'Intro' },
				{ startTime: 62.5, title: 'Thema' },
				{ startTime: 100, title: 'Werbung', toc: false },
				{ startTime: 'kaputt', title: 'Ungültig' }
			]
		});
		expect(parseChaptersJson(json)).toEqual([
			{ start: 0, title: 'Intro' },
			{ start: 62.5, title: 'Thema' }
		]);
	});

	it('returns nothing for a document that is not chapters at all', () => {
		expect(parseChaptersJson('nicht json')).toEqual([]);
		expect(parseChaptersJson('{"foo":1}')).toEqual([]);
	});
});
