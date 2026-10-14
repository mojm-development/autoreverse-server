import { describe, it, expect } from 'vitest';
import { writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { items as itemsTable, tracks as tracksTable } from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _streamGetHandler, _streamHeadHandler } from '../../src/routes/tracks/[id]/stream/+server';

describe('GET /tracks/{id}/stream', () => {
	it('serves 206 with the right byte range, headers, and exact byte content', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-track-'));
			const path = join(dir, 'a.mp3');
			// Create file with known content: each byte i contains value i%256
			const testContent = Buffer.alloc(1000);
			for (let i = 0; i < 1000; i++) {
				testContent[i] = i % 256;
			}
			writeFileSync(path, testContent);
			const [album] = await db
				.insert(itemsTable)
				.values({ kind: 'album', title: 'X', sortTitle: 'x' })
				.returning();
			const [trackRow] = await db
				.insert(tracksTable)
				.values({ itemId: album.id, position: 1, path, duration: 10 })
				.returning();

			const res = await callRoute(_streamGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) },
				headers: { range: 'bytes=100-199' }
			});
			expect(res.status).toBe(206);
			expect(res.headers.get('content-range')).toBe('bytes 100-199/1000');
			expect(res.headers.get('content-length')).toBe('100');
			const body = await res.arrayBuffer();
			expect(body.byteLength).toBe(100);
			// Verify actual byte content matches expected slice
			const view = new Uint8Array(body);
			for (let i = 0; i < 100; i++) {
				expect(view[i]).toBe((100 + i) % 256);
			}
		});
	});

	it('404s with "Unbekannter Track" for a missing row', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const res = await callRoute(_streamGetHandler, {
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
			const res = await callRoute(_streamGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) }
			});
			expect(res.status).toBe(410);
			expect((await res.json()).detail).toBe('Datei nicht mehr vorhanden');
		});
	});

	it('HEAD /tracks/{id}/stream returns 206 headers with no body', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-track-'));
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

			const res = await callRoute(_streamHeadHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) },
				headers: { range: 'bytes=100-199' }
			});
			expect(res.status).toBe(206);
			expect(res.headers.get('content-range')).toBe('bytes 100-199/1000');
			expect(res.headers.get('content-length')).toBe('100');
			expect(res.body).toBeNull();
		});
	});

	it('HEAD with no Range header returns 200 with no body', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-track-'));
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

			const res = await callRoute(_streamHeadHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) }
			});
			expect(res.status).toBe(200);
			expect(res.headers.get('content-length')).toBe('1000');
			expect(res.headers.get('accept-ranges')).toBe('bytes');
			expect(res.body).toBeNull();
		});
	});

	it('GET with unsatisfiable range returns 416 with content-range header', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-track-'));
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

			const res = await callRoute(_streamGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) },
				headers: { range: 'bytes=1000-1999' }
			});
			expect(res.status).toBe(416);
			expect(res.headers.get('content-range')).toBe('bytes */1000');
			expect(res.body).toBeNull();
		});
	});

	it('HEAD with unsatisfiable range returns 416 with content-range header and no body', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const dir = mkdtempSync(join(tmpdir(), 'autoreverse-track-'));
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

			const res = await callRoute(_streamHeadHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(trackRow.id) },
				headers: { range: 'bytes=1000-1999' }
			});
			expect(res.status).toBe(416);
			expect(res.headers.get('content-range')).toBe('bytes */1000');
			expect(res.body).toBeNull();
		});
	});
}, 60_000);
