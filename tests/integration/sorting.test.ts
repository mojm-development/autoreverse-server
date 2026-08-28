import { describe, it, expect } from 'vitest';
import { sql } from 'drizzle-orm';
import { withTestDb } from '../fixtures';
import { items as itemsTable } from '../../src/lib/server/db/schema';
import { items, seriesSiblings } from '../../src/lib/server/library/queries';
import { episodeNumber, naturalKey } from '../../src/lib/server/library/sorting';
import type { DrizzleDb } from '../../src/lib/server/db';

/** The scanner stores `sort_title` as the lowercased title, so the fixtures do too. */
async function seed(
	db: DrizzleDb,
	kind: 'book' | 'album',
	titles: string[],
	overrides: Partial<typeof itemsTable.$inferInsert> = {}
) {
	for (const title of titles) {
		await db
			.insert(itemsTable)
			.values({ kind, title, sortTitle: title.toLowerCase(), ...overrides });
	}
}

describe('natural sort', () => {
	it('orders embedded numbers numerically, not character by character', async () => {
		await withTestDb(async (db) => {
			// The exact shape of a TKKG library: plain lower(sort_title) puts
			// '100/' between '10/' and '11/', because '0' < '1' at index two.
			await seed(db, 'album', [
				'11/Elf',
				'2/Zwei',
				'100/Hundert',
				'10/Zehn',
				'1/Eins',
				'20/Zwanzig'
			]);

			const rows = await items(db, { kind: 'album', limit: 50, offset: 0, sort: 'title' });
			expect(rows.map((r) => r.title)).toEqual([
				'1/Eins',
				'2/Zwei',
				'10/Zehn',
				'11/Elf',
				'20/Zwanzig',
				'100/Hundert'
			]);
		});
	});

	it('still sorts alphabetically where there are no numbers', async () => {
		await withTestDb(async (db) => {
			await seed(db, 'album', ['Zenit', 'Ärger', 'Halbschatten', 'nordlicht']);
			const rows = await items(db, { kind: 'album', limit: 50, offset: 0, sort: 'title' });
			// 'Ä' collates after 'z' under the C/UTF-8 default the test container
			// uses, so assert only the ASCII-titled subsequence's relative order.
			const ascii = rows.map((r) => r.title).filter((t) => !t.startsWith('Ä'));
			expect(ascii).toEqual(['Halbschatten', 'nordlicht', 'Zenit']);
		});
	});

	it('is defined for a title that is only digits, and for an empty one', async () => {
		await withTestDb(async (db) => {
			await seed(db, 'album', ['7', '']);
			// coalesce to '{}' rather than NULL, so an empty title sorts first
			// instead of landing wherever the NULLS default puts it.
			const rows = await db.execute(
				sql`SELECT ${naturalKey(itemsTable.sortTitle)} AS key FROM items ORDER BY 1`
			);
			const keys = rows as unknown as Array<{ key: string[] }>;
			expect(keys).toHaveLength(2);
			expect(keys[0].key).toEqual([]);
			expect(keys[1].key).toEqual(['000000000007']);
		});
	});
});

describe('episodeNumber', () => {
	async function numberFor(db: DrizzleDb, title: string): Promise<number | null> {
		const rows = await db.execute(sql`SELECT ${episodeNumber(sql`${title}::text`)} AS n`);
		return (rows as unknown as Array<{ n: number | null }>)[0].n;
	}

	it('reads the instalment number out of a labelled title', async () => {
		await withTestDb(async (db) => {
			expect(await numberFor(db, 'Die neuen Fälle, Fall 04: Der Fluch')).toBe(4);
			expect(await numberFor(db, 'Sherlock Holmes - Die neuen Fälle, Fall 04: Der Fluch')).toBe(4);
			expect(await numberFor(db, 'Folge 12 – Das Erbe')).toBe(12);
			expect(await numberFor(db, 'Teil 3')).toBe(3);
			expect(await numberFor(db, 'Nr. 5 Der Hafen')).toBe(5);
			expect(await numberFor(db, 'Vol. 2')).toBe(2);
		});
	});

	it('reads a leading number used as the marker', async () => {
		await withTestDb(async (db) => {
			expect(await numberFor(db, '04/Der Fluch')).toBe(4);
			expect(await numberFor(db, '117 - Im Schloß der schlafenden Vampire')).toBe(117);
			expect(await numberFor(db, '08. Das Phantom')).toBe(8);
		});
	});

	it('does not invent a number where the digits mean something else', async () => {
		await withTestDb(async (db) => {
			// A separator is required after a leading number, so a title that
			// merely starts with one is not instalment 1984.
			expect(await numberFor(db, '1984')).toBeNull();
			expect(await numberFor(db, 'Nordlicht')).toBeNull();
			// The label must start a word: 'Zufall'/'Anteil' must not match
			// 'fall'/'teil'.
			expect(await numberFor(db, 'Ein Zufall 3')).toBeNull();
			expect(await numberFor(db, 'Der Anteil 2')).toBeNull();
			// ...and the digits must follow the label directly, so a longer word
			// that merely starts with one ('Volumen' vs 'vol'/'volume') is out.
			expect(await numberFor(db, 'Volumen 5')).toBeNull();
			// Grouped numbers are not instalments. Both of these were read as
			// 100 and 1 before the separator class dropped the comma and the
			// digit lookahead went in.
			expect(await numberFor(db, "100,000,000 Bon Jovi Fans Can't Be Wrong")).toBeNull();
			expect(await numberFor(db, '1.000 Meilen unter dem Meer')).toBeNull();
			expect(await numberFor(db, '100% Ginuwine')).toBeNull();
		});
	});
});

describe("sort: 'series'", () => {
	it('runs a series in instalment order despite inconsistent title prefixes', async () => {
		await withTestDb(async (db) => {
			// Exactly the library that prompted this: some folders carry the
			// series name alone in their album tag, others prefix the author.
			// Alphabetically these are two blocks that no title sort can rejoin.
			await seed(
				db,
				'book',
				[
					'Die neuen Fälle, Fall 01: Besuche eines Gentlemans',
					'Die neuen Fälle, Fall 30: Das Rätsel der Aurora',
					'Sherlock Holmes - Die neuen Fälle, Fall 04: Der Fluch',
					'Sherlock Holmes - Die neuen Fälle, Fall 05: Die Bestie',
					'Die neuen Fälle, Fall 02: Die Gesellschaft der Verlorenen'
				],
				{ author: 'Sherlock Holmes' }
			);

			const rows = await items(db, { kind: 'book', limit: 50, offset: 0, sort: 'series' });
			expect(rows.map((r) => r.title)).toEqual([
				'Die neuen Fälle, Fall 01: Besuche eines Gentlemans',
				'Die neuen Fälle, Fall 02: Die Gesellschaft der Verlorenen',
				'Sherlock Holmes - Die neuen Fälle, Fall 04: Der Fluch',
				'Sherlock Holmes - Die neuen Fälle, Fall 05: Die Bestie',
				'Die neuen Fälle, Fall 30: Das Rätsel der Aurora'
			]);
		});
	});

	it('groups by author before instalment number', async () => {
		await withTestDb(async (db) => {
			await seed(db, 'book', ['Fall 02: B'], { author: 'Alpha' });
			await seed(db, 'book', ['Fall 01: A'], { author: 'Beta' });
			const rows = await items(db, { kind: 'book', limit: 50, offset: 0, sort: 'series' });
			// Beta's Fall 01 must not jump ahead of Alpha's Fall 02.
			expect(rows.map((r) => r.author)).toEqual(['Alpha', 'Beta']);
		});
	});

	it('puts unnumbered titles after the numbered ones, alphabetically', async () => {
		await withTestDb(async (db) => {
			await seed(db, 'book', ['Fall 02: B', 'Zwischenspiel', 'Fall 01: A', 'Anhang'], {
				author: 'Alpha'
			});
			const rows = await items(db, { kind: 'book', limit: 50, offset: 0, sort: 'series' });
			expect(rows.map((r) => r.title)).toEqual([
				'Fall 01: A',
				'Fall 02: B',
				'Anhang',
				'Zwischenspiel'
			]);
		});
	});
});

describe('seriesSiblings', () => {
	it('orders a series drill-down by instalment number', async () => {
		await withTestDb(async (db) => {
			await seed(db, 'book', ['Fall 10: Zehn', 'Fall 2: Zwei', 'Fall 1: Eins'], {
				series: 'Die neuen Fälle'
			});
			const rows = await seriesSiblings(db, 'Die neuen Fälle');
			expect(rows.map((r) => r.title)).toEqual(['Fall 1: Eins', 'Fall 2: Zwei', 'Fall 10: Zehn']);
		});
	});
});
