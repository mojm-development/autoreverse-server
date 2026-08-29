import { describe, it, expect, vi, afterEach } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { callRoute } from './_callRoute';
import { _podcastsPreviewPostHandler } from '../../src/routes/podcasts/preview/+server';

const FEED = `<?xml version="1.0"?><rss version="2.0"><channel>
<title>Maschinenraum</title>
<description>Ein Podcast über Technik.</description>
<item><title>Folge 118</title><guid>ep-118</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate><enclosure url="https://x/118.mp3"/></item>
<item><title>Folge 117</title><guid>ep-117</guid><pubDate>Mon, 25 Dec 2025 10:00:00 GMT</pubDate><enclosure url="https://x/117.mp3"/></item>
</channel></rss>`;

describe('POST /podcasts/preview', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('returns title, description and episodes without subscribing', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_podcastsPreviewPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			const body = await res.json();

			expect(res.status).toBe(200);
			expect(body.title).toBe('Maschinenraum');
			expect(body.description).toBe('Ein Podcast über Technik.');
			expect(body.episodes.map((e: { title: string }) => e.title)).toEqual([
				'Folge 118',
				'Folge 117'
			]);
			expect(body.episodes[0].published_at).toBe('2026-01-01T10:00:00Z');
		});
	});

	it('leaves the library untouched — a preview is not a subscription', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await callRoute(_podcastsPreviewPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			const { items } = await import('../../src/lib/server/library/queries');
			expect(await items(db, { limit: 50, offset: 0 })).toHaveLength(0);
		});
	});

	it('rejects a missing feed_url with 422', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_podcastsPreviewPostHandler, {
				db,
				locals: { userId, token: null },
				body: {}
			});
			expect(res.status).toBe(422);
		});
	});

	it('turns an unreachable feed into 502, not a crash', async () => {
		vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('ECONNREFUSED')));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_podcastsPreviewPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://nope/feed.xml' }
			});
			expect(res.status).toBe(502);
		});
	});

	it('rejects an anonymous caller', async () => {
		await withTestDb(async (db) => {
			const res = await callRoute(_podcastsPreviewPostHandler, {
				db,
				locals: { userId: null, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			expect(res.status).toBe(401);
		});
	});
});
