import type { RequestHandler, RequestEvent } from '@sveltejs/kit';
import { extname, resolve, relative } from 'node:path';
import { stat, readFile } from 'node:fs/promises';
import { db as defaultDb, type DrizzleDb } from '$lib/server/db';
import { requireApiUser } from '$lib/server/auth/session';
import { item, coverPathFor } from '$lib/server/library/queries';
import { loadConfig } from '$lib/server/config';
import { getLibraryPaths } from '$lib/server/settings/libraryPaths';
import { apiError } from '$lib/server/api/error';
import { ApiError } from '$lib/server/api/errors';

const COVER_TYPES: Record<string, string> = {
	'.jpg': 'image/jpeg',
	'.jpeg': 'image/jpeg',
	'.png': 'image/png',
	'.webp': 'image/webp',
	'.gif': 'image/gif'
};

function isInside(path: string, root: string): boolean {
	const rel = relative(resolve(root), resolve(path));
	return !rel.startsWith('..') && !resolve(rel).includes('..');
}

export async function _coverGetHandler(
	db: DrizzleDb,
	event: Pick<RequestEvent, 'locals' | 'params'>
): Promise<Response> {
	try {
		requireApiUser(event.locals);
		const row = await item(db, Number(event.params.id));
		if (!row) return apiError(404, 'Unbekanntes Item');
		const coverPath = await coverPathFor(db, row);
		if (!coverPath) return apiError(404, 'Kein Cover');

		const config = loadConfig(process.env as Record<string, string | undefined>);
		const paths = await getLibraryPaths(db);
		const roots = [paths.booksDir, paths.musicDir, config.coverDir].filter(
			(r): r is string => r !== null
		);
		if (!roots.some((root) => isInside(coverPath, root))) return apiError(404, 'Kein Cover');

		let stats;
		try {
			stats = await stat(coverPath);
		} catch {
			return apiError(404, 'Kein Cover');
		}
		const contentType = COVER_TYPES[extname(coverPath).toLowerCase()] ?? 'application/octet-stream';
		const data = await readFile(coverPath);
		return new Response(data, {
			headers: {
				'content-type': contentType,
				'content-length': String(stats.size),
				'cache-control': 'private, max-age=86400'
			}
		});
	} catch (e) {
		if (e instanceof ApiError) return apiError(e.status, e.detail, e.retryAfter);
		throw e;
	}
}

export const GET: RequestHandler = (event) => _coverGetHandler(defaultDb, event);
