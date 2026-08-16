import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _coverGetHandler } from '../../src/routes/items/[id]/cover/+server';

describe('cover streaming', () => {
	let booksDir: string;
	let musicDir: string;
	let dataDir: string;
	const originalEnv = process.env;

	beforeEach(() => {
		// Create temp directories for the test
		booksDir = mkdtempSync(join(tmpdir(), 'capstan-books-'));
		musicDir = mkdtempSync(join(tmpdir(), 'capstan-music-'));
		dataDir = mkdtempSync(join(tmpdir(), 'capstan-data-'));
		mkdirSync(join(dataDir, 'covers'), { recursive: true });

		// Set env vars for loadConfig
		process.env.CAPSTAN_BOOKS = booksDir;
		process.env.CAPSTAN_MUSIC = musicDir;
		process.env.CAPSTAN_DATA = dataDir;
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

	it('refuses a cover_path outside the configured roots with 404 "Kein Cover"', async () => {
		await withTestDb(async (db) => {
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
