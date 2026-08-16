import { describe, it, expect } from 'vitest';
import { loadConfig } from '$lib/server/config';

describe('loadConfig', () => {
	it('applies defaults for CAPSTAN_DATA and DATABASE_URL', () => {
		const config = loadConfig({});
		expect(config.dataDir).toBe('./data');
		expect(config.coverDir).toBe('./data/covers');
		expect(config.podcastsDir).toBe('./data/podcasts');
		expect(config.databaseUrl).toBe('postgresql://capstan:capstan@localhost:5434/capstan');
	});

	it('honors explicit CAPSTAN_DATA and DATABASE_URL', () => {
		const config = loadConfig({
			CAPSTAN_DATA: '/data',
			DATABASE_URL: 'postgresql://x/y'
		});
		expect(config.coverDir).toBe('/data/covers');
		expect(config.databaseUrl).toBe('postgresql://x/y');
	});
});
