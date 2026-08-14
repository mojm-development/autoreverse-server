import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import {
	items as itemsTable,
	tracks as tracksTable,
	playbackSessions,
	listenEvents
} from '../../src/lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { callRoute } from './_callRoute';
import { playPostHandler } from '../../src/routes/play/[itemId]/+server';
import { sessionClosePostHandler } from '../../src/routes/sessions/[sessionId]/close/+server';
import { progressPutHandler } from '../../src/routes/progress/[itemId]/+server';

describe('playback API', () => {
	it('POST /play/{id} opens a session and returns tracks/chapters', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'X', sortTitle: 'x' })
				.returning();
			await db
				.insert(tracksTable)
				.values({ itemId: book.id, position: 1, path: '/a', duration: 120 });
			const res = await callRoute(playPostHandler, {
				db,
				locals: { userId, token: null },
				params: { itemId: String(book.id) }
			});
			const body = await res.json();
			expect(body.session_id).toBeTruthy();
			expect(body.start_position).toBe(0);
			expect(body.tracks).toHaveLength(1);
		});
	});

	it('POST /play/{id} 409s for an episode with no downloaded tracks', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'P', sortTitle: 'p' })
				.returning();
			const [episode] = await db
				.insert(itemsTable)
				.values({ kind: 'episode', parentId: podcast.id, title: 'E', sortTitle: 'e' })
				.returning();
			const res = await callRoute(playPostHandler, {
				db,
				locals: { userId, token: null },
				params: { itemId: String(episode.id) }
			});
			expect(res.status).toBe(409);
			expect((await res.json()).detail).toBe('Folge ist noch nicht heruntergeladen');
		});
	});

	it('POST /sessions/{id}/close records a listen_event for distance traveled, not wall-clock', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'X', sortTitle: 'x' })
				.returning();
			const playRes = await callRoute(playPostHandler, {
				db,
				locals: { userId, token: null },
				params: { itemId: String(book.id) }
			});
			const { session_id } = await playRes.json();
			await callRoute(sessionClosePostHandler, {
				db,
				locals: { userId, token: null },
				params: { sessionId: session_id },
				body: { position: 42, duration: 999 }
			});
			const [session] = await db
				.select()
				.from(playbackSessions)
				.where(eq(playbackSessions.id, session_id));
			expect(session.closedAt).not.toBeNull();
			const [event] = await db.select().from(listenEvents).where(eq(listenEvents.userId, userId));
			expect(event.seconds).toBe(42);
		});
	});

	it('PUT /progress/{id} upserts position and finished', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [book] = await db
				.insert(itemsTable)
				.values({ kind: 'book', title: 'X', sortTitle: 'x' })
				.returning();
			await callRoute(progressPutHandler, {
				db,
				locals: { userId, token: null },
				params: { itemId: String(book.id) },
				body: { position: 100, finished: true }
			});
			await callRoute(progressPutHandler, {
				db,
				locals: { userId, token: null },
				params: { itemId: String(book.id) },
				body: { position: 200, finished: false }
			});
			const { progress } = await import('../../src/lib/server/library/queries');
			expect(await progress(db, userId, book.id)).toEqual({ position: 200, finished: false });
		});
	});
}, 60_000);
