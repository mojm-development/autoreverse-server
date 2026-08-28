export interface ScanReport {
	new: number;
	updated: number;
	unchanged: number;
	missing: number;
	skipped: number;
}
export interface ScanProgress {
	phase: 'scanning' | 'storing';
	/** Which library root is being worked on — a scan walks books and music as
	 * two separate passes, so processed/total restart once per phase and the
	 * bar would otherwise appear to jump backwards for no visible reason. */
	root: string | null;
	/** Null while a phase is still counting what it has to do. */
	total: number | null;
	processed: number;
	new: number;
	updated: number;
	unchanged: number;
	skipped: number;
}

export type ProgressFn = (processed: number, total: number) => void;

/** The storing phase reports its running counts too, so the card can show
 * numbers climbing rather than a bar moving against nothing. */
export type StoreProgressFn = (
	processed: number,
	total: number,
	counts: { new: number; updated: number; unchanged: number; skipped: number }
) => void;
export interface ScanState {
	running: boolean;
	startedAt: string | null;
	finishedAt: string | null;
	lastReport: ScanReport | null;
	lastError: string | null;
	lastSkipped: string[];
	cancelled: boolean;
	cancelRequested: boolean;
	progress: ScanProgress | null;
}

/** One instance per server process — mirrors app.state.scan_state /
 * app.state.scan_lock being created once per FastAPI app instance, not per
 * request. Node has no GIL-adjacent concern here (single-threaded event
 * loop), so a plain module-level object stands in for the Python
 * threading.Lock-guarded dataclass; mutations below are synchronous property
 * assignments, which is enough serialization on a single event loop.
 *
 * CONCURRENCY CONSTRAINT FOR E2E TESTS: any e2e spec that triggers a real
 * POST /scan (like tests/e2e/smoke.e2e.ts) must avoid running concurrently
 * with another spec doing the same. This can be enforced via test ordering,
 * `--workers=1` for such specs, or by not adding more real-scan specs without
 * addressing this constraint. Simultaneous real scans have been empirically
 * shown to race under high test-runner concurrency. The same applies to the
 * library_config DB singleton (which scanState effectively guards via timing).
 */
export const scanState: ScanState = {
	running: false,
	startedAt: null,
	finishedAt: null,
	lastReport: null,
	lastError: null,
	lastSkipped: [],
	cancelled: false,
	cancelRequested: false,
	progress: null
};

/** Copies progress too: it is mutated in place while a scan runs, and a
 * shallow spread would hand the caller a live object that keeps changing
 * underneath the JSON serializer. */
export function snapshot(): ScanState {
	return { ...scanState, progress: scanState.progress ? { ...scanState.progress } : null };
}
