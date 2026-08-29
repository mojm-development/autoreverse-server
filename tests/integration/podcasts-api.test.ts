import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _podcastsPostHandler } from '../../src/routes/podcasts/+server';
import { _podcastDeleteHandler } from '../../src/routes/podcasts/[id]/+server';
import { _podcastRefreshPostHandler } from '../../src/routes/podcasts/[id]/refresh/+server';
import { _episodeDownloadPostHandler } from '../../src/routes/episodes/[id]/download/+server';

const FEED = `<?xml version="1.0"?><rss version="2.0"><channel><title>Maschinenraum</title>
<item><title>Folge 118</title><guid>ep-118</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate><enclosure url="https://x/118.mp3"/></item>
</channel></rss>`;

describe('podcasts API routes', () => {
	let booksDir: string;
	let musicDir: string;
	let dataDir: string;
	const originalEnv = process.env;

	beforeEach(() => {
		booksDir = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		musicDir = mkdtempSync(join(tmpdir(), 'autoreverse-music-'));
		dataDir = mkdtempSync(join(tmpdir(), 'autoreverse-data-'));
		process.env.AUTOREVERSE_DATA = dataDir;
	});

	afterEach(() => {
		process.env = originalEnv;
		vi.unstubAllGlobals();
		try {
			rmSync(booksDir, { recursive: true, force: true });
			rmSync(musicDir, { recursive: true, force: true });
			rmSync(dataDir, { recursive: true, force: true });
		} catch {
			// ignore cleanup errors
		}
	});

	it('POST /podcasts subscribes and returns the PodcastOut shape', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(_podcastsPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			expect(res.status).toBe(200);
			const body = await res.json();
			expect(body).toMatchObject({
				title: 'Maschinenraum',
				feed_url: 'https://x/feed.xml',
				// I-4 fix: real counts from syncEpisodes, not hardcoded zeros — this
				// feed has exactly one <item>, freshly inserted.
				new_episodes: 1,
				updated_episodes: 0
			});
			expect(body.id).toBeTruthy();
		});
	});

	it('POST /podcasts 502s when the feed fetch fails', async () => {
		vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(_podcastsPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			expect(res.status).toBe(502);
		});
	});

	it('POST /podcasts 422s when the fetched body is not parseable RSS/Atom', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({ ok: true, text: async () => '<html>not a feed</html>' })
		);
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(_podcastsPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			expect(res.status).toBe(422);
		});
	});

	// Subscribing is an ordinary user action, not administration. Admin rights guard
	// the library scan and user management; a podcast someone adds for themselves is
	// neither, and the native clients offer it on every account.
	it('a non-admin may subscribe via POST /podcasts', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			await createUser(db, 'admin', 'hunter2hunter2', true); // first user is always admin — throwaway
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', false);
			const res = await callRoute(_podcastsPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			expect(res.status).toBe(200);
		});
	});

	it('DELETE /podcasts/{id} returns 200 with the UnsubscribeOut shape (not 204)', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const subscribeRes = await callRoute(_podcastsPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			const podcastId = (await subscribeRes.json()).id;
			const res = await callRoute(_podcastDeleteHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(podcastId) }
			});
			expect(res.status).toBe(200);
			const body = await res.json();
			expect(body).toEqual({ episodes: 1, files_deleted: 0, files_kept: 0 });
		});
	});

	it('POST /podcasts/{id}/refresh 422s (not 404) when an existing podcast now serves an unparseable feed', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const subscribeRes = await callRoute(_podcastsPostHandler, {
				db,
				locals: { userId, token: null },
				body: { feed_url: 'https://x/feed.xml' }
			});
			const podcastId = (await subscribeRes.json()).id;
			vi.stubGlobal(
				'fetch',
				vi.fn().mockResolvedValue({ ok: true, text: async () => '<html>not a feed</html>' })
			);
			const res = await callRoute(_podcastRefreshPostHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(podcastId) }
			});
			expect(res.status).toBe(422);
		});
	});

	it('DELETE /podcasts/{id} 404s for an unknown podcast', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(_podcastDeleteHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999999' }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Unbekannter Podcast');
		});
	});

	it('POST /podcasts/{id}/refresh 404s for an unknown podcast', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(_podcastRefreshPostHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999999' }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Unbekannter Podcast');
		});
	});

	it('POST /episodes/{id}/download 404s at the route level for an unknown episode', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const res = await callRoute(_episodeDownloadPostHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999999' }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Unbekannte Folge');
		});
	});

	it('POST /episodes/{id}/download 422s when the episode has no media_url', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2', true);
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'P', sortTitle: 'p' })
				.returning();
			const [episode] = await db
				.insert(itemsTable)
				.values({ kind: 'episode', parentId: podcast.id, title: 'E', sortTitle: 'e', guid: 'g1' })
				.returning();
			const res = await callRoute(_episodeDownloadPostHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(episode.id) }
			});
			expect(res.status).toBe(422);
		});
	});
}, 60_000);
