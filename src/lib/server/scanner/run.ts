import { scanBooks } from './books';
import { scanMusic } from './music';
import { knownFiles, storeItems, markMissing } from './store';
import { scanState } from '../admin/scanState';
import { loadConfig } from '../config';
import { db } from '../db';

/** Two fully separate scanBooks/storeItems and scanMusic/storeItems passes
 * with DISTINCT root arguments — deliberately never merged. `root` gates
 * markMissing's scope; merging or swapping roots would mark the other
 * library's items as missing. Matches _run_scan's own emphasis on this. */
export async function runScan(): Promise<void> {
	const config = loadConfig(process.env as Record<string, string | undefined>);
	scanState.startedAt = new Date().toISOString();
	scanState.finishedAt = null;
	scanState.cancelled = false;
	scanState.lastError = null;
	const skipped: string[] = [];

	try {
		for (const [root, scan] of [
			[config.booksDir, scanBooks],
			[config.musicDir, scanMusic]
		] as const) {
			if (scanState.cancelRequested) {
				scanState.cancelled = true;
				break;
			}
			const known = await knownFiles(db);
			const scanned = await scan(root, known);
			const report = await storeItems(db, scanned, root, config.coverDir);
			const found = new Set(scanned.map((s) => s.sourcePath));
			const missing = await markMissing(db, root, found, skipped);
			scanState.lastReport = {
				new: (scanState.lastReport?.new ?? 0) + report.new,
				updated: (scanState.lastReport?.updated ?? 0) + report.updated,
				unchanged: (scanState.lastReport?.unchanged ?? 0) + report.unchanged,
				missing: (scanState.lastReport?.missing ?? 0) + missing,
				skipped: skipped.length
			};
		}
	} catch (e: unknown) {
		scanState.lastError = e instanceof Error ? e.message : String(e);
	} finally {
		scanState.running = false;
		scanState.finishedAt = new Date().toISOString();
		scanState.lastSkipped = skipped;
	}
}
