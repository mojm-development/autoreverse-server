import { describe, it, expect, vi, afterEach } from 'vitest';
import { searchDirectory } from '../../src/lib/server/podcasts/directory';

describe('searchDirectory', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('short-circuits to [] for an empty/whitespace term without an HTTP call', async () => {
		const fetchSpy = vi.fn();
		vi.stubGlobal('fetch', fetchSpy);
		expect(await searchDirectory('   ')).toEqual([]);
		expect(fetchSpy).not.toHaveBeenCalled();
	});

	it('maps iTunes search results, dropping entries without a feedUrl', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				json: async () => ({
					results: [
						{
							collectionName: 'Maschinenraum',
							artistName: 'M. Raum',
							feedUrl: 'https://x/feed.xml',
							artworkUrl100: 'https://x/art.png',
							trackCount: 118,
							genres: ['Technology']
						},
						{ collectionName: 'No Feed' } // no feedUrl — must be dropped
					]
				})
			})
		);
		const results = await searchDirectory('maschinenraum');
		expect(results).toHaveLength(1);
		expect(results[0]).toEqual({
			name: 'Maschinenraum',
			author: 'M. Raum',
			feedUrl: 'https://x/feed.xml',
			artworkUrl: 'https://x/art.png',
			episodeCount: 118,
			genre: 'Technology'
		});
	});

	it('uses DEFAULT_COUNTRY=DE and DEFAULT_LIMIT=25 when not overridden', async () => {
		const fetchSpy = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ results: [] }) });
		vi.stubGlobal('fetch', fetchSpy);
		await searchDirectory('x');
		const url = new URL(fetchSpy.mock.calls[0][0]);
		expect(url.searchParams.get('country')).toBe('DE');
		expect(url.searchParams.get('limit')).toBe('25');
	});
});
