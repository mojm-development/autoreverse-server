import { describe, it, expect } from 'vitest';
import { parseFeed } from '../../src/lib/server/podcasts/feed';

const SAMPLE_RSS = `<?xml version="1.0"?>
<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" version="2.0"><channel>
<title>Maschinenraum</title>
<description>&lt;p&gt;Ein Podcast   über  Technik&lt;/p&gt;</description>
<itunes:image href="https://example.com/cover (2).jpg"/>
<item>
  <title>&lt;b&gt;Der lange Weg&lt;/b&gt; zum Netz</title>
  <guid>ep-118</guid>
  <description>Wie das Netz entstand</description>
  <itunes:duration>1:02:03</itunes:duration>
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
		const emptyTitleRss = SAMPLE_RSS.replace(
			'<title>&lt;b&gt;Der lange Weg&lt;/b&gt; zum Netz</title>',
			'<title></title>'
		);
		const parsed = await parseFeed(emptyTitleRss);
		expect(parsed.episodes[0].title).toBe('Ohne Titel');
	});

	it('reads the channel description, HTML-stripped and whitespace-collapsed', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.description).toBe('Ein Podcast über Technik');
	});

	it('prefers <itunes:image> for the feed artwork', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.imageUrl).toBe('https://example.com/cover (2).jpg');
	});

	it('falls back to null for description and artwork when the feed has neither', async () => {
		const bare = `<?xml version="1.0"?><rss version="2.0"><channel><title>Leer</title></channel></rss>`;
		const parsed = await parseFeed(bare);
		expect(parsed.description).toBeNull();
		expect(parsed.imageUrl).toBeNull();
	});

	it('normalises <itunes:duration> H:MM:SS to seconds', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[0].durationSeconds).toBe(3723);
	});

	it('accepts a plain-seconds <itunes:duration> and rejects a malformed one', async () => {
		const seconds = SAMPLE_RSS.replace('<itunes:duration>1:02:03', '<itunes:duration>930');
		expect((await parseFeed(seconds)).episodes[0].durationSeconds).toBe(930);
		const bogus = SAMPLE_RSS.replace('<itunes:duration>1:02:03', '<itunes:duration>eine Stunde');
		expect((await parseFeed(bogus)).episodes[0].durationSeconds).toBeNull();
	});

	it('reads an episode description, null when absent', async () => {
		const parsed = await parseFeed(SAMPLE_RSS);
		expect(parsed.episodes[0].description).toBe('Wie das Netz entstand');
		expect(parsed.episodes[1].description).toBeNull();
	});
});
