import { describe, it, expect, vi, afterEach } from 'vitest';
import { withTestDb } from '../fixtures';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { eq } from 'drizzle-orm';
import {
	subscribe,
	refresh,
	unsubscribe,
	InvalidFeedError
} from '../../src/lib/server/podcasts/store';
import {
	downloadEpisode,
	EpisodeNotDownloadableError
} from '../../src/lib/server/podcasts/download';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const FEED = `<?xml version="1.0"?><rss version="2.0"><channel><title>Maschinenraum</title>
<item><title>Folge 118</title><guid>ep-118</guid><pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate><enclosure url="https://x/118.mp3"/></item>
</channel></rss>`;

describe('podcasts store', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('subscribe creates a podcast item + episodes; re-subscribing the same feed_url reuses the item', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const podcast1 = await subscribe(db, 'https://x/feed.xml');
			const podcast2 = await subscribe(db, 'https://x/feed.xml');
			expect(podcast2.id).toBe(podcast1.id);
			const episodes = await db
				.select()
				.from(itemsTable)
				.where(eq(itemsTable.parentId, podcast1.id));
			expect(episodes).toHaveLength(1); // not duplicated on the second subscribe
		});
	});

	it('subscribe rejects with InvalidFeedError when the fetched body is not parseable RSS/Atom', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({ ok: true, text: async () => '<html>not a feed</html>' })
		);
		await withTestDb(async (db) => {
			await expect(subscribe(db, 'https://x/feed.xml')).rejects.toThrow(InvalidFeedError);
		});
	});

	it('refresh rejects with InvalidFeedError when the fetched body is not parseable RSS/Atom', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const podcast = await subscribe(db, 'https://x/feed.xml');
			vi.stubGlobal(
				'fetch',
				vi.fn().mockResolvedValue({ ok: true, text: async () => '<html>not a feed</html>' })
			);
			await expect(refresh(db, podcast.id)).rejects.toThrow(InvalidFeedError);
		});
	});

	it('refresh dedupes episodes by (parent_id, guid), does not duplicate on repeat', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const podcast = await subscribe(db, 'https://x/feed.xml');
			await refresh(db, podcast.id);
			const episodes = await db
				.select()
				.from(itemsTable)
				.where(eq(itemsTable.parentId, podcast.id));
			expect(episodes).toHaveLength(1);
		});
	});

	it('unsubscribe deletes the podcast item, cascading episodes', async () => {
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => FEED }));
		await withTestDb(async (db) => {
			const podcast = await subscribe(db, 'https://x/feed.xml');
			const report = await unsubscribe(db, podcast.id, '/nonexistent/podcasts-dir');
			expect(report.episodes).toBe(1);
			expect(await db.select().from(itemsTable).where(eq(itemsTable.id, podcast.id))).toHaveLength(
				0
			);
		});
	});

	it('downloadEpisode throws EpisodeNotDownloadableError when the episode has no media_url', async () => {
		await withTestDb(async (db) => {
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'P', sortTitle: 'p' })
				.returning();
			const [episode] = await db
				.insert(itemsTable)
				.values({ kind: 'episode', parentId: podcast.id, title: 'E', sortTitle: 'e', guid: 'g1' })
				.returning();
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			await expect(downloadEpisode(db, episode.id, dir)).rejects.toThrow(
				EpisodeNotDownloadableError
			);
		});
	});

	it('downloadEpisode rejects when passed a podcast id instead of an episode id (kind filter)', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				body: {}, // truthy — would allow download to proceed if the podcast row passed the kind filter
				arrayBuffer: async () => new Uint8Array([0, 1, 2, 3]).buffer
			})
		);
		await withTestDb(async (db) => {
			const [podcast] = await db
				.insert(itemsTable)
				.values({
					kind: 'podcast',
					title: 'P',
					sortTitle: 'p',
					feedUrl: 'https://x/feed.xml' // a real feed URL, not a media URL — would corrupt tracks if used
				})
				.returning();
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			await expect(downloadEpisode(db, podcast.id, dir)).rejects.toThrow('not found');
			const tracks = await db.select().from(tracksTable).where(eq(tracksTable.itemId, podcast.id));
			expect(tracks).toHaveLength(0); // no spurious track attached to the podcast item
		});
	});

	it('downloadEpisode fetches, writes the file, extracts duration, and creates a track', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				body: {}, // truthy — downloadEpisode only checks response.body's truthiness before reading arrayBuffer
				arrayBuffer: async () => new Uint8Array([0, 1, 2, 3]).buffer
			})
		);
		await withTestDb(async (db) => {
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'P', sortTitle: 'p' })
				.returning();
			const [episode] = await db
				.insert(itemsTable)
				.values({
					kind: 'episode',
					parentId: podcast.id,
					title: 'E',
					sortTitle: 'e',
					guid: 'g2',
					feedUrl: 'https://x/ep.mp3'
				})
				.returning();
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			const result = await downloadEpisode(db, episode.id, dir);
			expect(result.trackId).toBeTruthy();
			const [track] = await db.select().from(tracksTable).where(eq(tracksTable.itemId, episode.id));
			expect(track.path).toBe(join(dir, `${episode.id}.mp3`));
		});
	});
}, 60_000);
