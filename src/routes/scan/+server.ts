import { json, type RequestHandler, type RequestEvent } from '@sveltejs/kit';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiAdmin } from '$lib/server/auth/session';
import { scanState, snapshot, type ScanState } from '$lib/server/admin/scanState';
import { runScan } from '$lib/server/scanner/run';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

export function _toWire(s: ScanState) {
	return {
		running: s.running,
		started_at: s.startedAt,
		finished_at: s.finishedAt,
		last_report: s.lastReport,
		last_error: s.lastError,
		last_skipped: s.lastSkipped,
		cancelled: s.cancelled,
		cancel_requested: s.cancelRequested,
		progress: s.progress
	};
}

export async function _scanPostHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals'>
): Promise<Response> {
	try {
		await requireApiAdmin(event.locals, db);
		if (scanState.running) throw new ApiError(409, 'Es läuft bereits ein Scan');
		scanState.running = true;
		scanState.cancelRequested = false;
		scanState.cancelled = false;
		scanState.progress = null;
		scanState.finishedAt = null;
		const snap = snapshot();
		void runScan(); // fire-and-forget, mirrors FastAPI's BackgroundTasks (runs after the response is sent)
		return json(_toWire(snap), { status: 202 });
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const POST: RequestHandler = (event) => _scanPostHandler(defaultDb, event);
