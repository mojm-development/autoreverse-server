export interface ScanReport {
	new: number;
	updated: number;
	unchanged: number;
	missing: number;
	skipped: number;
}
export interface ScanProgress {
	phase: 'scanning' | 'storing';
	root: string | null;
	total: number | null;
	processed: number;
	new: number;
	updated: number;
	unchanged: number;
	skipped: number;
}

export type ProgressFn = (processed: number, total: number) => void;

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

export function snapshot(): ScanState {
	return { ...scanState, progress: scanState.progress ? { ...scanState.progress } : null };
}
