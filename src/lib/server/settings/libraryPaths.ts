import { eq, sql } from 'drizzle-orm';
import { libraryConfig } from '../db/schema';
import type { DrizzleDb } from '../db';

export interface LibraryPaths {
	booksDir: string | null;
	musicDir: string | null;
}

const ROW_ID = 1;

export async function getLibraryPaths(db: DrizzleDb): Promise<LibraryPaths> {
	const [row] = await db.select().from(libraryConfig).where(eq(libraryConfig.id, ROW_ID));
	return { booksDir: row?.booksDir ?? null, musicDir: row?.musicDir ?? null };
}

export async function setLibraryPaths(
	db: DrizzleDb,
	paths: { booksDir: string; musicDir: string }
): Promise<void> {
	await db
		.insert(libraryConfig)
		.values({ id: ROW_ID, booksDir: paths.booksDir, musicDir: paths.musicDir })
		.onConflictDoUpdate({
			target: libraryConfig.id,
			set: { booksDir: paths.booksDir, musicDir: paths.musicDir, updatedAt: sql`now()` }
		});
}
