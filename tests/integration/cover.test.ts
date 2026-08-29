import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { setLibraryPaths } from '../../src/lib/server/settings/libraryPaths';
import { callRoute } from './_callRoute';
import { _coverGetHandler } from '../../src/routes/items/[id]/cover/+server';

describe('cover streaming', () => {
	let booksDir: string;
	let musicDir: string;
	let dataDir: string;
	const originalEnv = process.env;

	beforeEach(() => {
		// Create temp directories for the test
		booksDir = mkdtempSync(join(tmpdir(), 'autoreverse-books-'));
		musicDir = mkdtempSync(join(tmpdir(), 'autoreverse-music-'));
		dataDir = mkdtempSync(join(tmpdir(), 'autoreverse-data-'));
		mkdirSync(join(dataDir, 'covers'), { recursive: true });

		// Set env vars for loadConfig
		process.env.AUTOREVERSE_DATA = dataDir;
	});

	afterEach(() => {
		// Restore env and clean up temp directories
		process.env = originalEnv;
		try {
			rmSync(booksDir, { recursive: true, force: true });
			rmSync(musicDir, { recursive: true, force: true });
			rmSync(dataDir, { recursive: true, force: true });
		} catch {
			// ignore cleanup errors
		}
	});

	it('serves an in-root cover with the right content-type and cache header', async () => {
		await withTestDb(async (db) => {
			await setLibraryPaths(db, { booksDir, musicDir });
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const coverPath = join(dataDir, 'covers', '1.png');
			writeFileSync(coverPath, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
			const [row] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x', coverPath })
				.returning();
			const res = await callRoute(_coverGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(row.id) }
			});
			expect(res.status).toBe(200);
			expect(res.headers.get('content-type')).toBe('image/png');
			expect(res.headers.get('cache-control')).toBe('private, max-age=86400');
		});
	});

	it("serves the show's artwork for an episode that has none of its own", async () => {
		await withTestDb(async (db) => {
			await setLibraryPaths(db, { booksDir, musicDir });
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const coverPath = join(dataDir, 'covers', 'show.png');
			writeFileSync(coverPath, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'Apfelfunk', sortTitle: 'apfelfunk', coverPath })
				.returning();
			const [episode] = await db
				.insert(itemsTable)
				.values({
					kind: 'episode',
					title: 'Folge 500',
					sortTitle: 'folge 500',
					parentId: podcast.id
				})
				.returning();

			const res = await callRoute(_coverGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(episode.id) }
			});
			// Without the fallback every episode plays behind an empty square.
			expect(res.status).toBe(200);
			expect(res.headers.get('content-type')).toBe('image/png');
		});
	});

	it('refuses a cover_path outside the configured roots with 404 "Kein Cover"', async () => {
		await withTestDb(async (db) => {
			await setLibraryPaths(db, { booksDir, musicDir });
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [row] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x', coverPath: '/etc/passwd' })
				.returning();
			const res = await callRoute(_coverGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(row.id) }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Kein Cover');
		});
	});
}, 60_000);
