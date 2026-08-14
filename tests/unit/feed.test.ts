import { describe, it, expect } from 'vitest';
import { parseFeed } from '../../src/lib/server/podcasts/feed';

const SAMPLE_RSS = `<?xml version="1.0"?>
<rss version="2.0"><channel>
<title>Maschinenraum</title>
<item>
  <title>&lt;b&gt;Der lange Weg&lt;/b&gt; zum Netz</title>
  <guid>ep-118</guid>
  <pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate>
  <enclosure url="https://example.com/118.mp3" type="audio/mpeg" length="123"/>
</item>
<item>
  <title>Ohne Guid</title>
  <pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate>
</item>
</channel></rss>`;

describe('parseFeed', () => {
	it('strips HTML tags and unescapes entities from titles', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[0].title).toBe('Der lange Weg zum Netz');
	});

	it('uses <guid> when present', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[0].guid).toBe('ep-118');
	});

	it('falls back to a generated sha256-based guid when <guid> is missing', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[1].guid).toMatch(/^generated:[a-f0-9]{64}$/);
	});

	it('reads the enclosure url as mediaUrl, null when absent', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[0].mediaUrl).toBe('https://example.com/118.mp3');
		expect(parsed.episodes[1].mediaUrl).toBeNull();
	});

	it('parses pubDate to a UTC Date', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[0].publishedAt?.toISOString()).toBe('2026-01-01T10:00:00.000Z');
	});

	it('falls back to "Ohne Titel" for an empty title', async () => {
		const emptyTitleRss = SAMPLE_RSS.replace('<title>&lt;b&gt;Der lange Weg&lt;/b&gt; zum Netz</title>', '<title></title>');
		const parsed = await parseFeed(emptyTitleRss);
		expect(parsed.episodes[0].title).toBe('Ohne Titel');
	});
});
