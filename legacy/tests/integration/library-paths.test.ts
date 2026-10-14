import { describe, it, expect } from 'vitest';
import { sql } from 'drizzle-orm';
import { withTestDb } from '../fixtures';
import { getLibraryPaths, setLibraryPaths } from '../../src/lib/server/settings/libraryPaths';
import { libraryConfig } from '../../src/lib/server/db/schema';

describe('library paths settings', () => {
	it('getLibraryPaths returns nulls on an empty table', async () => {
		await withTestDb(async (db) => {
			expect(await getLibraryPaths(db)).toEqual({ booksDir: null, musicDir: null });
		});
	});

	it('setLibraryPaths then getLibraryPaths round-trips correctly', async () => {
		await withTestDb(async (db) => {
			await setLibraryPaths(db, { booksDir: '/library/books', musicDir: '/library/music' });
			expect(await getLibraryPaths(db)).toEqual({
				booksDir: '/library/books',
				musicDir: '/library/music'
			});
		});
	});

	it('calling setLibraryPaths twice updates the existing row rather than creating a second one', async () => {
		await withTestDb(async (db) => {
			await setLibraryPaths(db, { booksDir: '/library/books', musicDir: '/library/music' });
			await setLibraryPaths(db, { booksDir: '/library/books2', musicDir: '/library/music2' });

			expect(await getLibraryPaths(db)).toEqual({
				booksDir: '/library/books2',
				musicDir: '/library/music2'
			});

			const [{ count }] = await db
				.select({ count: sql<number>`count(*)::int` })
				.from(libraryConfig);
			expect(count).toBe(1);
		});
	});
});
