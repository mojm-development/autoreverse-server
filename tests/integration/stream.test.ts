import { describe, it, expect } from 'vitest';
import { writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { streamGetHandler } from '../../src/routes/tracks/[id]/stream/+server';

describe('GET /tracks/{id}/stream', () => {
	it('serves 206 with the right byte range and headers', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const dir = mkdtempSync(join(tmpdir(), 'capstan-track-'));
			const path = join(dir, 'a.mp3');
			writeFileSync(path, Buffer.alloc(1000, 1));
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x' })
				.returning();
			const [trackRow] = await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 1, path, duration: 10 })
				.returning();

			const res = await callRoute(streamGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) },
				headers: { range: 'bytes=0-99' }
			});
			expect(res.status).toBe(206);
			expect(res.headers.get('content-range')).toBe('bytes 0-99/1000');
			expect((await res.arrayBuffer()).byteLength).toBe(100);
		});
	});

	it('404s with "Unbekannter Track" for a missing row', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(streamGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: '999' }
			});
			expect(res.status).toBe(404);
			expect((await res.json()).detail).toBe('Unbekannter Track');
		});
	});

	it('410s with "Datei nicht mehr vorhanden" when the DB row exists but the file is gone', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x' })
				.returning();
			const [trackRow] = await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 1, path: '/nonexistent/x.mp3', duration: 10 })
				.returning();
			const res = await callRoute(streamGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) }
			});
			expect(res.status).toBe(410);
			expect((await res.json()).detail).toBe('Datei nicht mehr vorhanden');
		});
	});
}, 60_000);
