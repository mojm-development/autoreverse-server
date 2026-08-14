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

interface RSSItem {
	guid?: string;
	link?: string;
	title?: string;
	pubDate?: string;
	enclosure?: {
		url?: string;
		$?: { url?: string };
	};
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

function guidFor(item: RSSItem, index: number): string {
	const raw = item.guid || item.link;
	if (raw) return String(raw);
	const digest = createHash('sha256')
		.update(`${index}:${item.title ?? ''}`)
		.digest('hex');
	return `generated:${digest}`;
}

const parser = new Parser({ customFields: { item: [['enclosure', 'enclosure']] } });

/** Async because rss-parser has no synchronous XML-parsing API (unlike Python's feedparser.parse). */
export async function parseFeed(xml: string): Promise<ParsedFeed> {
	const parsed = await parser.parseString(xml);
	const episodes = (parsed.items ?? []).map((item: RSSItem, index: number) => ({
		guid: guidFor(item, index),
		title: cleanTitle(item.title),
		publishedAt: item.pubDate ? new Date(item.pubDate) : null,
		mediaUrl: item.enclosure?.url ?? item.enclosure?.$?.url ?? null
	}));
	return { title: cleanTitle(parsed.title), episodes };
}
