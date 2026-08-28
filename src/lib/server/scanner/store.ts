import { and, eq, isNotNull, isNull, notInArray, sql } from 'drizzle-orm';
import {
	items as itemsTable,
	tracks as tracksTable,
	chapters as chaptersTable
} from '../db/schema';
import { findCoverFile, extractEmbedded } from './covers';
import type { ScannedItem, ScanFailure } from './books';
import type { DrizzleDb } from '../db';

export async function knownFiles(db: DrizzleDb): Promise<Record<string, [number, number]>> {
	const rows = await db
		.select({ path: tracksTable.path, mtime: tracksTable.mtime, size: tracksTable.size })
		.from(tracksTable)
		.where(and(isNotNull(tracksTable.mtime), isNotNull(tracksTable.size)));
	const result: Record<string, [number, number]> = {};
	for (const row of rows) result[row.path] = [row.mtime!, row.size!];
	return result;
}

async function resolveCover(
	db: DrizzleDb,
	itemId: number,
	dir: string,
	firstTrackPaths: string[],
	coversDir: string,
	currentCoverPath: string | null
) {
	if (currentCoverPath) {
		try {
			await (await import('node:fs/promises')).stat(currentCoverPath);
			return currentCoverPath; // already resolved and still on disk — skip re-computation
		} catch {
			/* fall through and re-resolve */
		}
	}
	const folderCover = await findCoverFile(dir);
	if (folderCover) return folderCover;
	const CANDIDATES = 3;
	for (const trackPath of firstTrackPaths.slice(0, CANDIDATES)) {
		const extracted = await extractEmbedded(trackPath, coversDir, itemId);
		if (extracted) return extracted;
	}
	return currentCoverPath;
}

type Outcome = 'new' | 'updated' | 'unchanged';

export async function storeItems(
	db: DrizzleDb,
	scanned: ScannedItem[],
	root: string,
	coversDir: string,
	failures: ScanFailure[] = []
): Promise<{ new: number; updated: number; unchanged: number; skipped: number }> {
	const report = { new: 0, updated: 0, unchanged: 0, skipped: 0 };
	for (const entry of scanned) {
		// Per folder, not per pass. Each entry is its own transaction already, so
		// a failure here has nothing half-written to leave behind — but an
		// exception escaping the loop used to abandon every folder after it too.
		try {
			// The transaction reports what it did; the counters move only once it
			// has committed. Incrementing inside would credit work that a later
			// statement in the same transaction rolls back.
			const outcome = await db.transaction(async (tx): Promise<Outcome> => {
				const [existing] = await tx
					.select()
					.from(itemsTable)
					.where(eq(itemsTable.sourcePath, entry.sourcePath));

				if (entry.unchanged) {
					if (existing) {
						const cover = await resolveCover(
							tx as unknown as DrizzleDb,
							existing.id,
							entry.sourcePath,
							[],
							coversDir,
							existing.coverPath
						);
						await tx
							.update(itemsTable)
							.set({ missingSince: null, coverPath: cover })
							.where(eq(itemsTable.id, existing.id));
					}
					return 'unchanged';
				}

				let itemId: number;
				let outcome: Outcome;
				if (existing) {
					await tx
						.update(itemsTable)
						.set({
							title: entry.title,
							sortTitle: entry.title.toLowerCase(),
							author: entry.author ?? null,
							artist: entry.artist ?? null,
							albumArtist: entry.albumArtist ?? null,
							series: entry.series ?? null,
							year: entry.year ?? null,
							missingSince: null
						})
						.where(eq(itemsTable.id, existing.id));
					itemId = existing.id;
					outcome = 'updated';
				} else {
					const [inserted] = await tx
						.insert(itemsTable)
						.values({
							kind: entry.kind,
							sourcePath: entry.sourcePath,
							title: entry.title,
							sortTitle: entry.title.toLowerCase(),
							author: entry.author ?? null,
							artist: entry.artist ?? null,
							albumArtist: entry.albumArtist ?? null,
							series: entry.series ?? null,
							year: entry.year ?? null
						})
						.returning({ id: itemsTable.id });
					itemId = inserted.id;
					outcome = 'new';
				}

				// Remove tracks gone from this folder BEFORE upserting the rest, to
				// avoid a transient UNIQUE(item_id, position) collision during
				// renumbering (same ordering as store.py::_upsert_item_with_tracks).
				const currentPaths = entry.tracks.map((t) => t.path);
				if (currentPaths.length > 0) {
					await tx
						.delete(tracksTable)
						.where(and(eq(tracksTable.itemId, itemId), notInArray(tracksTable.path, currentPaths)));
				} else {
					await tx.delete(tracksTable).where(eq(tracksTable.itemId, itemId));
				}

				// Push every surviving track out of positive-position space before
				// assigning final positions below. Without this, a same-item
				// position swap among tracks that all still exist (e.g. an ID3 tag
				// fix flips two track numbers -- no file added/removed, so the
				// delete step above frees nothing) can hit a transient
				// UNIQUE(item_id, position) collision, since Postgres gives no
				// row-processing-order guarantee across the per-row upserts below.
				// A track's own id is globally unique, so negating it as a
				// sentinel position can never collide with a real 1-based target
				// position or another track's sentinel.
				await tx
					.update(tracksTable)
					.set({ position: sql`-${tracksTable.id}` })
					.where(eq(tracksTable.itemId, itemId));

				for (const t of entry.tracks) {
					await tx
						.insert(tracksTable)
						.values({
							itemId,
							position: t.position,
							path: t.path,
							duration: t.duration,
							title: t.title,
							disc: t.disc,
							mtime: t.mtime,
							size: t.size
						})
						.onConflictDoUpdate({
							target: tracksTable.path,
							set: {
								itemId,
								position: t.position,
								duration: t.duration,
								title: t.title,
								disc: t.disc,
								mtime: t.mtime,
								size: t.size
							}
						});
				}

				await tx.delete(chaptersTable).where(eq(chaptersTable.itemId, itemId));
				if (entry.chapters.length > 0) {
					await tx.insert(chaptersTable).values(
						entry.chapters.map((c, i) => ({
							itemId,
							position: i + 1,
							title: c.title,
							start: c.start,
							end: c.end
						}))
					);
				}

				const cover = await resolveCover(
					tx as unknown as DrizzleDb,
					itemId,
					entry.sourcePath,
					currentPaths,
					coversDir,
					existing?.coverPath ?? null
				);
				await tx.update(itemsTable).set({ coverPath: cover }).where(eq(itemsTable.id, itemId));
				return outcome;
			});
			report[outcome] += 1;
		} catch (e) {
			failures.push({
				path: entry.sourcePath,
				message: e instanceof Error ? e.message : String(e)
			});
			report.skipped += 1;
		}
	}
	return report;
}

/** Path-PREFIX (not LIKE) comparison, trailing slash included so `/x/lib`
 * doesn't match `/x/library/...` — direct port of the `substr(...)  = prefix`
 * logic in store.py::_mark_missing. Only touches rows with missing_since
 * IS NULL (already-missing rows are never re-touched — this is the fix for
 * the old 50%-threshold bug where re-marked items polluted their own
 * denominator on a second run). */
export async function markMissing(
	db: DrizzleDb,
	root: string,
	found: Set<string>,
	skippedPaths: string[]
): Promise<number> {
	const prefix = `${root}/`;
	const isSkipped = (path: string) =>
		skippedPaths.some((entry) => path === entry || path.startsWith(`${entry}/`));

	const candidates = await db
		.select({ id: itemsTable.id, sourcePath: itemsTable.sourcePath })
		.from(itemsTable)
		.where(
			and(
				sql`substr(${itemsTable.sourcePath}, 1, ${prefix.length}) = ${prefix}`,
				isNull(itemsTable.missingSince)
			)
		);

	let count = 0;
	await db.transaction(async (tx) => {
		for (const candidate of candidates) {
			if (
				candidate.sourcePath &&
				(found.has(candidate.sourcePath) || isSkipped(candidate.sourcePath))
			)
				continue;
			await tx
				.update(itemsTable)
				.set({ missingSince: sql`now()` })
				.where(eq(itemsTable.id, candidate.id));
			count += 1;
		}
	});
	return count;
}
