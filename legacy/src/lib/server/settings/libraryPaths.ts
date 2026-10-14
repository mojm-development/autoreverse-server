import { eq, sql } from 'drizzle-orm';
import { libraryConfig } from '../db/schema';
import type { DrizzleDb } from '../db';

export interface LibraryPaths {
	booksDir: string | null;
	musicDir: string | null;
}

const ROW_ID = 1;

export function normalizeDir(path: string): string {
	const trimmed = path.trim();
	const stripped = trimmed.replace(/\/+$/, '');
	return stripped || (trimmed ? '/' : '');
}

export async function getLibraryPaths(db: DrizzleDb): Promise<LibraryPaths> {
	const [row] = await db.select().from(libraryConfig).where(eq(libraryConfig.id, ROW_ID));
	return {
		booksDir: row?.booksDir ? normalizeDir(row.booksDir) : null,
		musicDir: row?.musicDir ? normalizeDir(row.musicDir) : null
	};
}

export async function setLibraryPaths(
	db: DrizzleDb,
	paths: { booksDir: string; musicDir: string }
): Promise<void> {
	const booksDir = normalizeDir(paths.booksDir);
	const musicDir = normalizeDir(paths.musicDir);
	await db
		.insert(libraryConfig)
		.values({ id: ROW_ID, booksDir, musicDir })
		.onConflictDoUpdate({
			target: libraryConfig.id,
			set: { booksDir, musicDir, updatedAt: sql`now()` }
		});
}
