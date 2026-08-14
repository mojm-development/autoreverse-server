import Parser from 'rss-parser';
import { createHash } from 'node:crypto';

export interface ParsedEpisode {
	guid: string;
	title: string;
	publishedAt: Date | null;
	mediaUrl: string | null;
}
export interface ParsedFeed {
	title: string;
	episodes: ParsedEpisode[];
}

function cleanTitle(raw: string | undefined): string {
	const stripped = (raw ?? '').replace(/<[^>]+>/g, '');
	const decoded = stripped
		.replaceAll('&lt;', '<')
		.replaceAll('&gt;', '>')
		.replaceAll('&amp;', '&')
		.replaceAll('&quot;', '"')
		.replaceAll('&#39;', "'")
		.trim();
	return decoded || 'Ohne Titel';
}

function guidFor(item: any, index: number): string {
	const raw = item.guid || item.link;
	if (raw) return String(raw);
	const digest = createHash('sha256').update(`${index}:${item.title ?? ''}`).digest('hex');
	return `generated:${digest}`;
}

const parser = new Parser({ customFields: { item: [['enclosure', 'enclosure']] } });

/** Synchronous wrapper over rss-parser's async API for call-site parity with
 * the Python feedparser.parse(raw) (also synchronous) — rss-parser's
 * parseString is async under the hood, so this is actually async; keep it
 * `async` here despite the type-signature-looking-sync docstring below and
 * update call sites (fetch.ts, Task 25) accordingly. */
export async function parseFeed(xml: string): Promise<ParsedFeed> {
	const parsed = await parser.parseString(xml);
	const episodes = (parsed.items ?? []).map((item: any, index: number) => ({
		guid: guidFor(item, index),
		title: cleanTitle(item.title),
		publishedAt: item.pubDate ? new Date(item.pubDate) : null,
		mediaUrl: item.enclosure?.url ?? item.enclosure?.$?.url ?? null
	}));
	return { title: cleanTitle(parsed.title), episodes };
}
