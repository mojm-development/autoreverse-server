import Parser from 'rss-parser';
import { createHash } from 'node:crypto';

export interface ParsedChapter {
	start: number;
	title: string;
}
export interface ParsedEpisode {
	guid: string;
	title: string;
	publishedAt: Date | null;
	mediaUrl: string | null;
	description: string | null;
	durationSeconds: number | null;
	/** Podlove Simple Chapters, carried inline in the feed. */
	chapters: ParsedChapter[];
	/** Podcasting 2.0 points at a JSON file instead; fetched at download time. */
	chaptersUrl: string | null;
}
export interface ParsedFeed {
	title: string;
	description: string | null;
	imageUrl: string | null;
	episodes: ParsedEpisode[];
}

type XmlNode = { $?: Record<string, string | undefined> } & Record<string, unknown>;

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
	pscChapters?: XmlNode;
	podcastChapters?: XmlNode;
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

/**
 * Podlove timestamps: "HH:MM:SS.mmm", "MM:SS", or "SS" — normal play time as
 * defined by the PSC spec, so the hour part is optional and the fraction is too.
 */
export function pscTimeToSeconds(raw: string | undefined | null): number | null {
	if (!raw) return null;
	const trimmed = raw.trim();
	if (!/^\d{1,3}(:\d{1,2}){0,2}([.,]\d{1,3})?$/.test(trimmed)) return null;
	const [whole, fraction] = trimmed.split(/[.,]/);
	const parts = whole.split(':').map(Number);
	if (parts.some((n) => !Number.isFinite(n))) return null;
	const seconds = parts.reduce((total, part) => total * 60 + part, 0);
	const millis = fraction ? Number(`0.${fraction}`) : 0;
	return seconds + millis;
}

function asArray(value: unknown): XmlNode[] {
	if (Array.isArray(value)) return value as XmlNode[];
	if (value && typeof value === 'object') return [value as XmlNode];
	return [];
}

/** Inline `<psc:chapters><psc:chapter start=… title=…/></psc:chapters>`. */
function chaptersOf(item: RSSItem): ParsedChapter[] {
	const container = item.pscChapters;
	if (!container) return [];
	const raw = asArray(container['psc:chapter'] ?? container.chapter);
	const chapters: ParsedChapter[] = [];
	for (const node of raw) {
		const attrs = node.$ ?? {};
		const start = pscTimeToSeconds(attrs.start);
		if (start === null) continue;
		chapters.push({ start, title: cleanTitle(attrs.title) });
	}
	// Feeds are not required to be in order, and a duplicate start would break the
	// chapter list's unique position index.
	chapters.sort((a, b) => a.start - b.start);
	return chapters.filter((c, i) => i === 0 || c.start !== chapters[i - 1].start);
}

function chaptersUrlOf(item: RSSItem): string | null {
	const url = item.podcastChapters?.$?.url;
	return url && url.trim() ? url.trim() : null;
}

function guidFor(item: RSSItem, index: number): string {
	const raw = item.guid || item.link;
	if (raw) return String(raw);
	const digest = createHash('sha256')
		.update(`${index}:${item.title ?? ''}`)
		.digest('hex');
	return `generated:${digest}`;
}

const parser = new Parser({
	customFields: {
		item: [
			['enclosure', 'enclosure'],
			['psc:chapters', 'pscChapters'],
			['podcast:chapters', 'podcastChapters']
		]
	}
});

export async function parseFeed(xml: string): Promise<ParsedFeed> {
	const parsed = await parser.parseString(xml);
	const episodes = (parsed.items ?? []).map((item: RSSItem, index: number) => ({
		guid: guidFor(item, index),
		title: cleanTitle(item.title),
		publishedAt: item.pubDate ? new Date(item.pubDate) : null,
		mediaUrl: item.enclosure?.url ?? item.enclosure?.$?.url ?? null,
		description: cleanText(item.contentSnippet ?? item.itunes?.summary ?? item.content),
		durationSeconds: durationToSeconds(item.itunes?.duration),
		chapters: chaptersOf(item),
		chaptersUrl: chaptersUrlOf(item)
	}));
	const channel = parsed as RSSChannel;
	return {
		title: cleanTitle(parsed.title),
		description: cleanText(channel.description),
		imageUrl: channel.itunes?.image ?? channel.image?.url ?? null,
		episodes
	};
}
