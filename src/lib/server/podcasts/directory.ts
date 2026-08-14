export const DEFAULT_COUNTRY = 'DE';
export const DEFAULT_LIMIT = 25;
const DIRECTORY_URL = 'https://itunes.apple.com/search';

export interface DirectoryResult {
	name: string;
	author: string;
	feedUrl: string;
	artworkUrl: string | null;
	episodeCount: number | null;
	genre: string | null;
}

export class DirectorySearchError extends Error {}

export async function searchDirectory(term: string, opts: { limit?: number; country?: string } = {}): Promise<DirectoryResult[]> {
	if (!term.trim()) return [];
	const url = new URL(DIRECTORY_URL);
	url.searchParams.set('media', 'podcast');
	url.searchParams.set('entity', 'podcast');
	url.searchParams.set('term', term);
	url.searchParams.set('limit', String(opts.limit ?? DEFAULT_LIMIT));
	url.searchParams.set('country', opts.country ?? DEFAULT_COUNTRY);

	const response = await fetch(url, { signal: AbortSignal.timeout(20_000) });
	let body: any;
	try {
		body = await response.json();
	} catch {
		throw new DirectorySearchError('Unerwartete Antwort des Podcast-Verzeichnisses');
	}
	if (!Array.isArray(body?.results)) throw new DirectorySearchError('Unerwartete Antwort des Podcast-Verzeichnisses');

	const results: DirectoryResult[] = [];
	for (const entry of body.results) {
		if (typeof entry !== 'object' || entry === null) continue;
		const feedUrl = entry.feedUrl;
		if (!feedUrl) continue;
		results.push({
			name: entry.collectionName || entry.trackName || feedUrl,
			author: entry.artistName || '',
			feedUrl,
			artworkUrl: entry.artworkUrl100 ?? entry.artworkUrl60 ?? null,
			episodeCount: entry.trackCount ?? null,
			genre: entry.genres?.[0] ?? null
		});
	}
	return results;
}
