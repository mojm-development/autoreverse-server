import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import {
	items as itemsTable,
	tracks as tracksTable,
	progress as progressTable
} from '../../src/lib/server/db/schema';
import { callRoute } from './_callRoute';
import { _itemGetHandler } from '../../src/routes/items/[id]/+server';
import { _itemsGetHandler } from '../../src/routes/items/+server';
import { _itemChildrenGetHandler } from '../../src/routes/items/[id]/children/+server';
import { _libraryMoreGetHandler } from '../../src/routes/library/more/+server';

describe('descriptions', () => {
	it('GET /items/{id} carries the description', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [row] = await db
				.insert(itemsTable)
				.values({
					kind: 'book',
					title: 'Der Schwarm',
					sortTitle: 'der schwarm',
					description: 'Das Meer schlägt zurück.'
				})
				.returning({ id: itemsTable.id });

			const res = await callRoute(_itemGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(row.id) }
			});

			expect((await res.json()).description).toBe('Das Meer schlägt zurück.');
		});
	});

	// Blurbs run long. A library page carrying one per row would be megabytes of prose
	// for rows that show nothing but a title and a cover.
	it('GET /items leaves the description out of the list', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			await db.insert(itemsTable).values({
				kind: 'book',
				title: 'Der Schwarm',
				sortTitle: 'der schwarm',
				description: 'Das Meer schlägt zurück.'
			});

			const res = await callRoute(_itemsGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/items'
			});

			expect((await res.json()).items[0]).not.toHaveProperty('description');
		});
	});
});

describe('GET /items/{id}/children', () => {
	it('reports each episode duration so a list can show it without opening anything', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const [podcast] = await db
				.insert(itemsTable)
				.values({ kind: 'podcast', title: 'Maschinenraum', sortTitle: 'maschinenraum' })
				.returning({ id: itemsTable.id });
			const [episode] = await db
				.insert(itemsTable)
				.values({
					kind: 'episode',
					parentId: podcast.id,
					title: 'Folge 118',
					sortTitle: 'folge 118'
				})
				.returning({ id: itemsTable.id });
			await db.insert(tracksTable).values([
				{ itemId: episode.id, position: 1, path: '/a.mp3', duration: 1200 },
				{ itemId: episode.id, position: 2, path: '/b.mp3', duration: 300 }
			]);

			const res = await callRoute(_itemChildrenGetHandler, {
				db,
				locals: { userId, token: null },
				params: { id: String(podcast.id) }
			});
			const body = await res.json();

			expect(body.items).toHaveLength(1);
			expect(body.durations[String(episode.id)]).toBe(1500);
		});
	});
});

describe('GET /library/more?sort=played', () => {
	it('puts the most recently played first and keeps the untouched ones', async () => {
		await withTestDb(async (db) => {
			const userId = await createUser(db, 'oliver', 'hunter2hunter2');
			const rows = await db
				.insert(itemsTable)
				.values([
					{ kind: 'album', title: 'Alt', sortTitle: 'alt' },
					{ kind: 'album', title: 'Neu', sortTitle: 'neu' },
					{ kind: 'album', title: 'Nie gehört', sortTitle: 'nie gehört' }
				])
				.returning({ id: itemsTable.id, title: itemsTable.title });
			const byTitle = Object.fromEntries(rows.map((r) => [r.title, r.id]));
			await db.insert(progressTable).values([
				{
					userId,
					itemId: byTitle['Alt'],
					position: 10,
					updatedAt: new Date(Date.now() - 86_400_000)
				},
				{ userId, itemId: byTitle['Neu'], position: 20, updatedAt: new Date() }
			]);

			const res = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId, token: null },
				url: 'http://test/library/more?kind=album&sort=played'
			});
			const body = await res.json();

			expect(body.items.map((i: { title: string }) => i.title)).toEqual([
				'Neu',
				'Alt',
				'Nie gehört'
			]);
		});
	});

	it('orders by the asking user, not by whoever listened last', async () => {
		await withTestDb(async (db) => {
			const mine = await createUser(db, 'oliver', 'hunter2hunter2');
			const theirs = await createUser(db, 'someone', 'hunter2hunter2');
			const rows = await db
				.insert(itemsTable)
				.values([
					{ kind: 'album', title: 'Meins', sortTitle: 'meins' },
					{ kind: 'album', title: 'Ihres', sortTitle: 'ihres' }
				])
				.returning({ id: itemsTable.id, title: itemsTable.title });
			const byTitle = Object.fromEntries(rows.map((r) => [r.title, r.id]));
			await db.insert(progressTable).values([
				{
					userId: mine,
					itemId: byTitle['Meins'],
					position: 5,
					updatedAt: new Date(Date.now() - 86_400_000)
				},
				{ userId: theirs, itemId: byTitle['Ihres'], position: 5, updatedAt: new Date() }
			]);

			const res = await callRoute(_libraryMoreGetHandler, {
				db,
				locals: { userId: mine, token: null },
				url: 'http://test/library/more?kind=album&sort=played'
			});
			const body = await res.json();

			expect(body.items[0].title).toBe('Meins');
		});
	});
});
