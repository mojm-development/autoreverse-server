import { describe, it, expect, vi, afterEach } from 'vitest';
import { withTestDb } from '../fixtures';
import {
	items as itemsTable,
	tracks as tracksTable,
	chapters as chaptersTable
} from '../../src/lib/server/db/schema';
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
import { relocateLegacyDownloads } from '../../src/lib/server/podcasts/relocate';
import { mkdtempSync, writeFileSync, existsSync } from 'node:fs';
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

	it('subscribe stores Podlove chapters from the feed and the 2.0 chapter URL for later', async () => {
		const feed = `<?xml version="1.0"?><rss version="2.0"
			xmlns:psc="http://podlove.org/simple-chapters"
			xmlns:podcast="https://podcastindex.org/namespace/1.0"
			xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel><title>Maschinenraum</title>
			<item><title>Mit Kapiteln</title><guid>ep-1</guid><enclosure url="https://x/1.mp3"/>
				<itunes:duration>00:30:00</itunes:duration>
				<psc:chapters version="1.2">
					<psc:chapter start="00:00:00" title="Begrüßung"/>
					<psc:chapter start="00:10:00" title="Thema"/>
				</psc:chapters></item>
			<item><title>Nur URL</title><guid>ep-2</guid><enclosure url="https://x/2.mp3"/>
				<podcast:chapters url="https://x/2.json" type="application/json+chapters"/></item>
		</channel></rss>`;
		vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => feed }));
		await withTestDb(async (db) => {
			const podcast = await subscribe(db, 'https://x/feed.xml');
			const episodes = await db
				.select()
				.from(itemsTable)
				.where(eq(itemsTable.parentId, podcast.id));

			const withChapters = episodes.find((e) => e.title === 'Mit Kapiteln')!;
			const rows = await db
				.select()
				.from(chaptersTable)
				.where(eq(chaptersTable.itemId, withChapters.id));
			expect(rows.map((r) => [r.position, r.title, r.start, r.end])).toEqual([
				[1, 'Begrüßung', 0, 600],
				// The last chapter runs to the length the feed declared.
				[2, 'Thema', 600, 1800]
			]);

			// The JSON document is not fetched on a feed refresh — one HTTP request per
			// episode would be absurd — so only its address is kept.
			const urlOnly = episodes.find((e) => e.title === 'Nur URL')!;
			expect(urlOnly.chaptersUrl).toBe('https://x/2.json');
			expect(
				await db.select().from(chaptersTable).where(eq(chaptersTable.itemId, urlOnly.id))
			).toHaveLength(0);
		});
	}, 60_000);

	it('subscribe stores the feed artwork under the covers dir and points cover_path at it', async () => {
		const FEED_WITH_ART = FEED.replace(
			'<channel><title>Maschinenraum</title>',
			'<channel><title>Maschinenraum</title><itunes:image href="https://x/art.png"/>'
		).replace(
			'<rss version="2.0">',
			'<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" version="2.0">'
		);
		// A Uint8Array built from a literal owns its ArrayBuffer; Buffer.from(...).buffer
		// would hand out Node's shared pool, whose leading bytes are not the PNG magic.
		const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00]);
		vi.stubGlobal(
			'fetch',
			vi.fn().mockImplementation(async (input: unknown) => {
				const url = String(input);
				if (url.endsWith('art.png')) return { ok: true, arrayBuffer: async () => png.buffer };
				return { ok: true, text: async () => FEED_WITH_ART };
			})
		);
		const coversDir = mkdtempSync(join(tmpdir(), 'covers-'));
		await withTestDb(async (db) => {
			const podcast = await subscribe(db, 'https://x/feed.xml', { coversDir });
			expect(podcast.coverPath).toBe(join(coversDir, `${podcast.id}.png`));
			const [row] = await db.select().from(itemsTable).where(eq(itemsTable.id, podcast.id));
			expect(row.coverPath).toBe(podcast.coverPath);
		});
	});

	it('subscribe still succeeds when the artwork cannot be fetched', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockImplementation(async (input: unknown) => {
				if (String(input).endsWith('art.png')) throw new Error('network down');
				return { ok: true, text: async () => FEED };
			})
		);
		const coversDir = mkdtempSync(join(tmpdir(), 'covers-'));
		await withTestDb(async (db) => {
			const podcast = await subscribe(db, 'https://x/feed.xml', { coversDir });
			expect(podcast.id).toBeGreaterThan(0);
			expect(podcast.coverPath).toBeNull();
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
			expect(track.path).toBe(join(dir, 'P', 'E.mp3')); // <podcastsDir>/<Podcast>/<Folge>.mp3
		});
	});

	it('downloadEpisode keeps two episodes with the same title apart', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				ok: true,
				body: {},
				arrayBuffer: async () => new Uint8Array([0, 1, 2, 3]).buffer
			})
		);
		await withTestDb(async (db) => {
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'Der / Podcast', sortTitle: 'der podcast' })
				.returning();
			const episodes: { id: number }[] = [];
			for (const guid of ['g3', 'g4']) {
				const [episode] = await db
					.insert(itemsTable)
					.values({
						kind: 'episode',
						parentId: podcast.id,
						title: 'Folge: eins?',
						sortTitle: 'folge eins',
						guid,
						feedUrl: 'https://x/ep.mp3'
					})
					.returning();
				episodes.push(episode);
			}
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			await downloadEpisode(db, episodes[0].id, dir);
			await downloadEpisode(db, episodes[1].id, dir);

			const paths = await db
				.select({ itemId: tracksTable.itemId, path: tracksTable.path })
				.from(tracksTable);
			const byItem = new Map(paths.map((row) => [row.itemId, row.path]));
			const folder = join(dir, 'Der Podcast'); // separators and ':' '?' scrubbed out of both names
			expect(byItem.get(episodes[0].id)).toBe(join(folder, 'Folge eins.mp3'));
			expect(byItem.get(episodes[1].id)).toBe(join(folder, `Folge eins (${episodes[1].id}).mp3`));
		});
	});

	/** An episode with its old-style flat download already on disk and in the tracks table. */
	async function legacyDownload(
		db: Parameters<Parameters<typeof withTestDb>[0]>[0],
		dir: string,
		titles: { podcast: string; episode: string },
		guid: string
	) {
		const [podcast] = await db
			.insert(itemsTable)
			.values({ kind: 'podcast', title: titles.podcast, sortTitle: titles.podcast.toLowerCase() })
			.returning();
		const [episode] = await db
			.insert(itemsTable)
			.values({
				kind: 'episode',
				parentId: podcast.id,
				title: titles.episode,
				sortTitle: titles.episode.toLowerCase(),
				guid,
				feedUrl: 'https://x/ep.mp3'
			})
			.returning();
		const path = join(dir, `${episode.id}.mp3`);
		writeFileSync(path, 'audio');
		const [track] = await db
			.insert(tracksTable)
			.values({ itemId: episode.id, position: 1, path, duration: 10, title: titles.episode })
			.returning();
		return { podcast, episode, track, path };
	}

	it('relocateLegacyDownloads moves an old flat download into its podcast folder', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			const { episode, path } = await legacyDownload(
				db,
				dir,
				{ podcast: 'Maschinenraum', episode: 'Folge 118' },
				'ep-118'
			);

			const result = await relocateLegacyDownloads(db, dir);

			expect(result).toEqual({ moved: 1, skipped: 0, failed: 0 });
			const moved = join(dir, 'Maschinenraum', 'Folge 118.mp3');
			expect(existsSync(moved)).toBe(true);
			expect(existsSync(path)).toBe(false); // the old flat file is gone, not copied
			const [track] = await db.select().from(tracksTable).where(eq(tracksTable.itemId, episode.id));
			expect(track.path).toBe(moved);
		});
	});

	it('relocateLegacyDownloads has nothing left to do on a second run', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			await legacyDownload(db, dir, { podcast: 'P', episode: 'E' }, 'ep-1');

			expect(await relocateLegacyDownloads(db, dir)).toEqual({ moved: 1, skipped: 0, failed: 0 });
			expect(await relocateLegacyDownloads(db, dir)).toEqual({ moved: 0, skipped: 1, failed: 0 });
			expect(existsSync(join(dir, 'P', 'E.mp3'))).toBe(true);
		});
	});

	it('relocateLegacyDownloads leaves a file outside the podcasts root where it is', async () => {
		await withTestDb(async (db) => {
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
			const elsewhere = mkdtempSync(join(tmpdir(), 'autoreverse-library-'));
			const { episode, track } = await legacyDownload(
				db,
				elsewhere,
				{ podcast: 'P', episode: 'E' },
				'ep-2'
			);

			const result = await relocateLegacyDownloads(db, dir);

			expect(result).toEqual({ moved: 0, skipped: 1, failed: 0 });
			expect(existsSync(track.path)).toBe(true);
			const [row] = await db.select().from(tracksTable).where(eq(tracksTable.itemId, episode.id));
			expect(row.path).toBe(track.path);
		});
	});
}, 60_000);
