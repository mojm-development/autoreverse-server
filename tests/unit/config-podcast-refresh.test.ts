import { describe, it, expect } from 'vitest';
import { loadConfig } from '../../src/lib/server/config';

describe('podcast refresh interval', () => {
	it('defaults to six hours', () => {
		expect(loadConfig({}).podcastRefreshHours).toBe(6);
	});

	it('is turned off by zero, and by anything that is not a number', () => {
		expect(loadConfig({ AUTOREVERSE_PODCAST_REFRESH_HOURS: '0' }).podcastRefreshHours).toBe(0);
		expect(loadConfig({ AUTOREVERSE_PODCAST_REFRESH_HOURS: 'nope' }).podcastRefreshHours).toBe(0);
		expect(loadConfig({ AUTOREVERSE_PODCAST_REFRESH_HOURS: '-3' }).podcastRefreshHours).toBe(0);
	});

	it('takes a custom interval', () => {
		expect(loadConfig({ AUTOREVERSE_PODCAST_REFRESH_HOURS: '12' }).podcastRefreshHours).toBe(12);
	});

	it('puts the artist images beside the other server-managed files', () => {
		expect(loadConfig({ AUTOREVERSE_DATA: '/data' }).artistsDir).toBe('/data/artists');
	});
});
