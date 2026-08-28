import { scanBooks, libraryRootProblem, type ScanFailure } from './books';
import { scanMusic } from './music';
import { knownFiles, storeItems, removeVanished } from './store';
import { scanState, type ScanReport, type ScanProgress } from '../admin/scanState';
import { loadConfig } from '../config';
import { getLibraryPaths } from '../settings/libraryPaths';
import { db } from '../db';

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
	scanState.lastReport = null;
	const skipped: string[] = [];
	const failures: ScanFailure[] = [];
	const rootErrors: string[] = [];
	const totals: ScanReport = { new: 0, updated: 0, unchanged: 0, removed: 0, skipped: 0 };

	try {
		for (const [root, scan] of [
			[paths.booksDir, scanBooks],
			[paths.musicDir, scanMusic]
		] as const) {
			if (scanState.cancelRequested) {
				scanState.cancelled = true;
				break;
			}
			const problem = await libraryRootProblem(root);
			if (problem) {
				rootErrors.push(`${root}: ${problem}`);
				continue;
			}

			const known = await knownFiles(db);
			const rootFailures: ScanFailure[] = [];

			const progress: ScanProgress = {
				phase: 'scanning',
				root,
				total: null,
				processed: 0,
				new: totals.new,
				updated: totals.updated,
				unchanged: totals.unchanged,
				skipped: totals.skipped
			};
			scanState.progress = progress;

			const scanned = await scan(root, known, rootFailures, (processed, total) => {
				progress.processed = processed;
				progress.total = total;
			});

			progress.phase = 'storing';
			progress.processed = 0;
			progress.total = scanned.length;
			const report = await storeItems(
				db,
				scanned,
				root,
				config.coverDir,
				rootFailures,
				(processed, total, counts) => {
					progress.processed = processed;
					progress.total = total;
					progress.new = totals.new + counts.new;
					progress.updated = totals.updated + counts.updated;
					progress.unchanged = totals.unchanged + counts.unchanged;
					progress.skipped = totals.skipped + counts.skipped;
				}
			);
			failures.push(...rootFailures);
			skipped.push(...rootFailures.map((f) => f.path));
			const found = new Set(scanned.map((s) => s.sourcePath));
			const removed = await removeVanished(db, root, found, skipped);
			totals.new += report.new;
			totals.updated += report.updated;
			totals.unchanged += report.unchanged;
			totals.removed += removed;
			totals.skipped = failures.length;
			scanState.lastReport = { ...totals };
		}
		if (rootErrors.length > 0) scanState.lastError = rootErrors.join(' · ');
	} catch (e: unknown) {
		scanState.lastError = e instanceof Error ? e.message : String(e);
	} finally {
		scanState.running = false;
		scanState.finishedAt = new Date().toISOString();
		scanState.progress = null;
		scanState.lastSkipped = failures.map((f) => `${f.path}: ${f.message}`);
	}
}
