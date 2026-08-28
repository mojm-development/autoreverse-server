import Parser from 'rss-parser';
import { createHash } from 'node:crypto';

export interface ParsedEpisode {
	guid: string;
	title: string;
	publishedAt: Date | null;
	mediaUrl: string | null;
	description: string | null;
	durationSeconds: number | null;
}
export interface ParsedFeed {
	title: string;
	description: string | null;
	imageUrl: string | null;
	episodes: ParsedEpisode[];
}

interface RSSItem {
	guid?: string;
	link?: string;
	title?: string;
	pubDate?: string;
	content?: string;
	contentSnippet?: string;
	itunes?: { duration?: string; summary?: string };
	enclosure?: {
		url?: string;
		$?: { url?: string };
	};
}

interface RSSChannel {
	description?: string;
	image?: { url?: string };
	itunes?: { image?: string };
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

function cleanText(raw: string | undefined): string | null {
	if (!raw) return null;
	const stripped = raw.replace(/<[^>]+>/g, ' ');
	const decoded = stripped
		.replaceAll('&lt;', '<')
		.replaceAll('&gt;', '>')
		.replaceAll('&amp;', '&')
		.replaceAll('&quot;', '"')
		.replaceAll('&#39;', "'")
		.replace(/\s+/g, ' ')
		.trim();
	return decoded || null;
}

function durationToSeconds(raw: string | undefined): number | null {
	if (!raw) return null;
	const parts = raw.trim().split(':');
	if (parts.some((p) => p === '' || !/^\d+$/.test(p))) return null;
	const seconds = parts.reduce((total, part) => total * 60 + Number(part), 0);
	return Number.isFinite(seconds) ? seconds : null;
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

export async function parseFeed(xml: string): Promise<ParsedFeed> {
	const parsed = await parser.parseString(xml);
	const episodes = (parsed.items ?? []).map((item: RSSItem, index: number) => ({
		guid: guidFor(item, index),
		title: cleanTitle(item.title),
		publishedAt: item.pubDate ? new Date(item.pubDate) : null,
		mediaUrl: item.enclosure?.url ?? item.enclosure?.$?.url ?? null,
		description: cleanText(item.contentSnippet ?? item.itunes?.summary ?? item.content),
		durationSeconds: durationToSeconds(item.itunes?.duration)
	}));
	const channel = parsed as RSSChannel;
	return {
		title: cleanTitle(parsed.title),
		description: cleanText(channel.description),
		imageUrl: channel.itunes?.image ?? channel.image?.url ?? null,
		episodes
	};
}
