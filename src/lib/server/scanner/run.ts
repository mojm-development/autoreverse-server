import { scanBooks, libraryRootProblem, type ScanFailure } from './books';
import { scanMusic } from './music';
import { knownFiles, storeItems, markMissing } from './store';
import { scanState, type ScanReport } from '../admin/scanState';
import { loadConfig } from '../config';
import { getLibraryPaths } from '../settings/libraryPaths';
import { db } from '../db';

/** Two fully separate scanBooks/storeItems and scanMusic/storeItems passes
 * with DISTINCT root arguments — deliberately never merged. `root` gates
 * markMissing's scope; merging or swapping roots would mark the other
 * library's items as missing. Matches _run_scan's own emphasis on this. */
export async function runScan(): Promise<void> {
	const config = loadConfig(process.env as Record<string, string | undefined>);
	const paths = await getLibraryPaths(db);
	if (!paths.booksDir || !paths.musicDir) {
		scanState.lastError =
			'Bibliothekspfade sind noch nicht konfiguriert (Einstellungen → Bibliotheken)';
		scanState.running = false;
		scanState.finishedAt = new Date().toISOString();
		return;
	}
	scanState.startedAt = new Date().toISOString();
	scanState.finishedAt = null;
	scanState.cancelled = false;
	scanState.lastError = null;
	// Was never cleared, so each run added its counts to the previous run's and
	// the numbers only ever grew. The per-root accumulation below relies on the
	// null, and `skipped` was already absolute — the report contradicted itself.
	scanState.lastReport = null;
	const skipped: string[] = [];
	const failures: ScanFailure[] = [];
	const rootErrors: string[] = [];
	const totals: ScanReport = { new: 0, updated: 0, unchanged: 0, missing: 0, skipped: 0 };

	try {
		for (const [root, scan] of [
			[paths.booksDir, scanBooks],
			[paths.musicDir, scanMusic]
		] as const) {
			if (scanState.cancelRequested) {
				scanState.cancelled = true;
				break;
			}
			// Before anything else: an unreadable root must never reach markMissing.
			// A library whose mount vanished scans as zero items, which is exactly
			// what a genuinely emptied library looks like — and markMissing would
			// dutifully flag every book in it as gone. Refusing to scan the root at
			// all is the only safe reading of "I cannot see it".
			const problem = await libraryRootProblem(root);
			if (problem) {
				rootErrors.push(`${root}: ${problem}`);
				continue;
			}

			const known = await knownFiles(db);
			// Per-root, then merged: `failures` spans both passes, and re-deriving
			// the skip list from it would hand the music pass the books' failures.
			const rootFailures: ScanFailure[] = [];
			const scanned = await scan(root, known, rootFailures);
			const report = await storeItems(db, scanned, root, config.coverDir, rootFailures);
			failures.push(...rootFailures);
			// A folder that failed keeps its existing rows: it was not observed to be
			// absent, only impossible to read.
			skipped.push(...rootFailures.map((f) => f.path));
			const found = new Set(scanned.map((s) => s.sourcePath));
			const missing = await markMissing(db, root, found, skipped);
			totals.new += report.new;
			totals.updated += report.updated;
			totals.unchanged += report.unchanged;
			totals.missing += missing;
			totals.skipped = failures.length;
			scanState.lastReport = { ...totals };
		}
		if (rootErrors.length > 0) scanState.lastError = rootErrors.join(' · ');
	} catch (e: unknown) {
		scanState.lastError = e instanceof Error ? e.message : String(e);
	} finally {
		scanState.running = false;
		scanState.finishedAt = new Date().toISOString();
		scanState.lastSkipped = failures.map((f) => `${f.path}: ${f.message}`);
	}
}
