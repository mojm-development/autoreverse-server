import { describe, it, expect, vi } from 'vitest';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { eq } from 'drizzle-orm';
import { withTestDb } from '../fixtures';
import {
	items as itemsTable,
	tracks as tracksTable,
	libraryConfig
} from '../../src/lib/server/db/schema';
import {
	applyRetention,
	getKeepDefault,
	setKeepDefault,
	setKeepForPodcast,
	effectiveKeep,
	retainForPodcast,
	refreshAllAndRetain
} from '../../src/lib/server/podcasts/retention';
import type { DrizzleDb } from '../../src/lib/server/db';

async function seedPodcast(db: DrizzleDb) {
	const [podcast] = await db
		.insert(itemsTable)
		.values({ kind: 'podcast', title: 'Show', sortTitle: 'show', feedUrl: 'https://x/feed' })
		.returning();
	return podcast;
}

async function seedEpisode(db: DrizzleDb, podcastId: number, day: number, file?: string) {
	const [episode] = await db
		.insert(itemsTable)
		.values({
			kind: 'episode',
			parentId: podcastId,
			title: `Folge ${day}`,
			sortTitle: `folge ${day}`,
			publishedAt: new Date(Date.UTC(2026, 0, day))
		})
		.returning();
	if (file) {
		await db
			.insert(tracksTable)
			.values({ itemId: episode.id, position: 1, path: file, duration: 60 });
	}
	return episode;
}

function tempFile(name: string) {
	const dir = mkdtempSync(join(tmpdir(), 'autoreverse-podcasts-'));
	const path = join(dir, name);
	writeFileSync(path, 'audio');
	return path;
}

describe('podcast retention', () => {
	it('deletes downloads past the newest N, keeping the newest', async () => {
		await withTestDb(async (db) => {
			const podcast = await seedPodcast(db);
			const oldFile = tempFile('old.mp3');
			const newFile = tempFile('new.mp3');
			const older = await seedEpisode(db, podcast.id, 1, oldFile);
			const newer = await seedEpisode(db, podcast.id, 9, newFile);

			const result = await applyRetention(db, podcast.id, 1, '/tmp');
			expect(result.freed).toBe(1);

			const remaining = await db.select().from(tracksTable);
			expect(remaining.map((t) => t.itemId)).toEqual([newer.id]);
			await expect(stat(oldFile)).rejects.toThrow();
			await expect(stat(newFile)).resolves.toBeTruthy();
			expect(older.id).toBeDefined();
		});
	});

	it('keeps everything when the number is high enough', async () => {
		await withTestDb(async (db) => {
			const podcast = await seedPodcast(db);
			await seedEpisode(db, podcast.id, 1, tempFile('a.mp3'));
			await seedEpisode(db, podcast.id, 2, tempFile('b.mp3'));

			const result = await applyRetention(db, podcast.id, 10, '/tmp');
			expect(result.freed).toBe(0);
			expect(await db.select().from(tracksTable)).toHaveLength(2);
		});
	});

	it('zero means keep nothing downloaded', async () => {
		await withTestDb(async (db) => {
			const podcast = await seedPodcast(db);
			await seedEpisode(db, podcast.id, 1, tempFile('a.mp3'));
			const result = await applyRetention(db, podcast.id, 0, '/tmp');
			expect(result.freed).toBe(1);
			expect(await db.select().from(tracksTable)).toHaveLength(0);
		});
	});

	it('counts a failed download instead of aborting the rest', async () => {
		await withTestDb(async (db) => {
			const podcast = await seedPodcast(db);
			await seedEpisode(db, podcast.id, 1);
			await seedEpisode(db, podcast.id, 2);
			const result = await applyRetention(db, podcast.id, 2, '/tmp');
			expect(result.failed).toBe(2);
			expect(result.downloaded).toBe(0);
		});
	});

	it('falls back to the global default and lets one podcast override it', async () => {
		await withTestDb(async (db) => {
			const podcast = await seedPodcast(db);
			expect(await getKeepDefault(db)).toBe(0);

			await setKeepDefault(db, 5);
			expect(await getKeepDefault(db)).toBe(5);
			expect(await effectiveKeep(db, { keepEpisodes: null })).toBe(5);
			expect(await effectiveKeep(db, { keepEpisodes: 2 })).toBe(2);

			await setKeepForPodcast(db, podcast.id, 3);
			const [row] = await db.select().from(itemsTable).where(eq(itemsTable.id, podcast.id));
			expect(row.keepEpisodes).toBe(3);

			await setKeepForPodcast(db, podcast.id, null);
			const [cleared] = await db.select().from(itemsTable).where(eq(itemsTable.id, podcast.id));
			expect(cleared.keepEpisodes).toBeNull();
		});
	});

	it('clamps a default that is out of range rather than storing it', async () => {
		await withTestDb(async (db) => {
			await setKeepDefault(db, 999);
			expect(await getKeepDefault(db)).toBe(50);
			await setKeepDefault(db, -4);
			expect(await getKeepDefault(db)).toBe(0);
			const rows = await db.select().from(libraryConfig);
			expect(rows).toHaveLength(1);
		});
	});

	it('retainForPodcast uses the override and ignores anything that is not a podcast', async () => {
		await withTestDb(async (db) => {
			const podcast = await seedPodcast(db);
			await setKeepDefault(db, 10);
			await setKeepForPodcast(db, podcast.id, 1);
			await seedEpisode(db, podcast.id, 1, tempFile('a.mp3'));
			await seedEpisode(db, podcast.id, 2, tempFile('b.mp3'));

			const result = await retainForPodcast(db, podcast.id, '/tmp');
			expect(result.freed).toBe(1);

			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'A', sortTitle: 'a' })
				.returning();
			expect(await retainForPodcast(db, album.id, '/tmp')).toEqual({
				downloaded: 0,
				freed: 0,
				failed: 0
			});
		});
	});
});

const ONE_EPISODE_RSS = `<?xml version="1.0"?>
<rss version="2.0"><channel>
<title>Maschinenraum</title>
<description>Ein Podcast</description>
<item>
  <title>Neue Folge</title>
  <guid>ep-1</guid>
  <pubDate>Mon, 01 Jan 2026 10:00:00 GMT</pubDate>
  <enclosure url="https://example.com/1.mp3" type="audio/mpeg" length="123"/>
</item>
</channel></rss>`;

describe('the scheduled refresh run', () => {
	it('counts the new episodes and names the feed that failed', async () => {
		await withTestDb(async (db) => {
			const reachable = await seedPodcast(db);
			const [unreachable] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'Weg', sortTitle: 'weg', feedUrl: 'https://y/feed' })
				.returning();

			vi.stubGlobal('fetch', async (input: unknown) => {
				if (String(input) === 'https://x/feed') return new Response(ONE_EPISODE_RSS);
				throw new Error('getaddrinfo ENOTFOUND y');
			});
			try {
				const result = await refreshAllAndRetain(db, {
					coversDir: '/tmp',
					podcastsDir: '/tmp'
				});
				expect(result.podcasts).toBe(2);
				expect(result.newEpisodes).toBe(1);
				expect(result.failed).toBe(1);
				expect(result.errors).toHaveLength(1);
				expect(result.errors[0]).toContain(String(unreachable.id));
				expect(result.errors[0]).toContain('ENOTFOUND');
			} finally {
				vi.unstubAllGlobals();
			}

			const episodes = await db
				.select()
				.from(itemsTable)
				.where(eq(itemsTable.parentId, reachable.id));
			expect(episodes).toHaveLength(1);
		});
	});
});
