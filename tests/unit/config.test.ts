import { describe, it, expect } from 'vitest';
import { loadConfig } from '$lib/server/config';

describe('loadConfig', () => {
	it('throws when CAPSTAN_BOOKS is missing', () => {
		expect(() => loadConfig({ CAPSTAN_MUSIC: '/music' })).toThrow('CAPSTAN_BOOKS');
	});

	it('throws when CAPSTAN_MUSIC is missing', () => {
		expect(() => loadConfig({ CAPSTAN_BOOKS: '/books' })).toThrow('CAPSTAN_MUSIC');
	});

	it('applies defaults for CAPSTAN_DATA and DATABASE_URL', () => {
		const config = loadConfig({ CAPSTAN_BOOKS: '/books', CAPSTAN_MUSIC: '/music' });
		expect(config.dataDir).toBe('./data');
		expect(config.coverDir).toBe('./data/covers');
		expect(config.podcastsDir).toBe('./data/podcasts');
		expect(config.databaseUrl).toBe('postgresql://capstan:capstan@localhost:5433/capstan');
	});

	it('honors explicit CAPSTAN_DATA and DATABASE_URL', () => {
		const config = loadConfig({
			CAPSTAN_BOOKS: '/books',
			CAPSTAN_MUSIC: '/music',
			CAPSTAN_DATA: '/data',
			DATABASE_URL: 'postgresql://x/y'
		});
		expect(config.coverDir).toBe('/data/covers');
		expect(config.databaseUrl).toBe('postgresql://x/y');
	});
});
