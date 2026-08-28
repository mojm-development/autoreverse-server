import { stat } from 'node:fs/promises';
import { and, eq, isNotNull, isNull, notInArray, sql } from 'drizzle-orm';
import {
	items as itemsTable,
	tracks as tracksTable,
	chapters as chaptersTable
} from '../db/schema';
import { findCoverFile, extractEmbedded } from './covers';
import type { ScannedItem, ScanFailure } from './books';
import type { DrizzleDb } from '../db';
import type { StoreProgressFn } from '../admin/scanState';

export async function knownFiles(db: DrizzleDb): Promise<Record<string, [number, number]>> {
	const rows = await db
		.select({ path: tracksTable.path, mtime: tracksTable.mtime, size: tracksTable.size })
		.from(tracksTable)
		.where(and(isNotNull(tracksTable.mtime), isNotNull(tracksTable.size)));
	const result: Record<string, [number, number]> = {};
	for (const row of rows) result[row.path] = [row.mtime!, row.size!];
	return result;
}

async function fileExists(path: string): Promise<boolean> {
	try {
		await stat(path);
		return true;
	} catch {
		return false;
	}
}

async function resolveCover(
	db: DrizzleDb,
	itemId: number,
	dir: string,
	firstTrackPaths: string[],
	coversDir: string,
	currentCoverPath: string | null
) {
	if (currentCoverPath && (await fileExists(currentCoverPath))) {
		return currentCoverPath;
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
	failures: ScanFailure[] = [],
	onProgress?: StoreProgressFn
): Promise<{ new: number; updated: number; unchanged: number; skipped: number }> {
	const report = { new: 0, updated: 0, unchanged: 0, skipped: 0 };
	for (const [i, entry] of scanned.entries()) {
		try {
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

				const currentPaths = entry.tracks.map((t) => t.path);
				if (currentPaths.length > 0) {
					await tx
						.delete(tracksTable)
						.where(and(eq(tracksTable.itemId, itemId), notInArray(tracksTable.path, currentPaths)));
				} else {
					await tx.delete(tracksTable).where(eq(tracksTable.itemId, itemId));
				}

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
		onProgress?.(i + 1, scanned.length, report);
	}
	return report;
}

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
				sql`(${itemsTable.sourcePath} = ${root} OR substr(${itemsTable.sourcePath}, 1, ${prefix.length}) = ${prefix})`,
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
