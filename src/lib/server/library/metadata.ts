import { randomUUID } from 'node:crypto';
import { eq } from 'drizzle-orm';
import { items as itemsTable, tracks as tracksTable, metadataEdits } from '../db/schema';
import { ApiError } from '../api/errors';
import type { DrizzleDb } from '../db';

/**
 * Hand edits to library metadata.
 *
 * Every field a person sets is remembered in `locked_fields`, because the scanner
 * rewrites title, author, artist, series and year from the files on every scan of a
 * changed folder — without the lock, a correction survives only until the next scan.
 * Setting a field locks it; resetting it hands the field back to the scanner.
 */

type FieldType = 'text' | 'int' | 'float';

interface FieldSpec {
	/** Column name in the Drizzle schema — this is what `locked_fields` stores. */
	column: string;
	type: FieldType;
	max?: number;
}

/** Wire name (snake_case, like the rest of the API) → column. */
export const ITEM_FIELDS: Record<string, FieldSpec> = {
	title: { column: 'title', type: 'text', max: 500 },
	sort_title: { column: 'sortTitle', type: 'text', max: 500 },
	author: { column: 'author', type: 'text', max: 300 },
	artist: { column: 'artist', type: 'text', max: 300 },
	album_artist: { column: 'albumArtist', type: 'text', max: 300 },
	narrator: { column: 'narrator', type: 'text', max: 300 },
	series: { column: 'series', type: 'text', max: 300 },
	series_index: { column: 'seriesIndex', type: 'float' },
	year: { column: 'year', type: 'int' }
};

export const TRACK_FIELDS: Record<string, FieldSpec> = {
	title: { column: 'title', type: 'text', max: 500 },
	disc: { column: 'disc', type: 'int' }
};

/** A title change carries its sort key along, unless that was edited on its own. */
const DERIVED: Record<string, string> = { title: 'sortTitle' };

function coerce(name: string, spec: FieldSpec, raw: unknown): string | number | null {
	if (raw === null) return null;
	if (spec.type === 'text') {
		if (typeof raw !== 'string') throw new ApiError(422, `${name} muss Text sein`);
		const value = raw.trim();
		if (value.length === 0) return null;
		if (spec.max && value.length > spec.max) {
			throw new ApiError(422, `${name} ist zu lang (max. ${spec.max} Zeichen)`);
		}
		return value;
	}
	const value = typeof raw === 'number' ? raw : Number(raw);
	if (!Number.isFinite(value)) throw new ApiError(422, `${name} muss eine Zahl sein`);
	if (spec.type === 'int') {
		if (!Number.isInteger(value)) throw new ApiError(422, `${name} muss eine ganze Zahl sein`);
		return value;
	}
	if (value < 0) throw new ApiError(422, `${name} darf nicht negativ sein`);
	return value;
}

function asText(value: unknown): string | null {
	return value === null || value === undefined ? null : String(value);
}

export interface EditRequest {
	/** Wire-named fields to set. `null` clears a field — and locks it empty. */
	set?: Record<string, unknown>;
	/** Wire-named fields to hand back to the scanner. */
	reset?: string[];
}

function parseRequest(
	body: unknown,
	fields: Record<string, FieldSpec>
): { set: Record<string, string | number | null>; reset: string[] } {
	if (!body || typeof body !== 'object') throw new ApiError(422, 'Ungültige Anfrage');
	const { set, reset } = body as EditRequest;
	const parsed: Record<string, string | number | null> = {};
	for (const [name, raw] of Object.entries(set ?? {})) {
		const spec = fields[name];
		if (!spec) throw new ApiError(422, `Unbekanntes Feld: ${name}`);
		parsed[spec.column] = coerce(name, spec, raw);
	}
	const resetColumns: string[] = [];
	for (const name of reset ?? []) {
		const spec = fields[name];
		if (!spec) throw new ApiError(422, `Unbekanntes Feld: ${name}`);
		resetColumns.push(spec.column);
	}
	if (Object.keys(parsed).length === 0 && resetColumns.length === 0) {
		throw new ApiError(422, 'Keine Änderungen angegeben');
	}
	return { set: parsed, reset: resetColumns };
}

export interface EditResult<T> {
	row: T;
	batchId: string;
	changed: string[];
}

export async function editItem(
	db: DrizzleDb,
	userId: number,
	itemId: number,
	body: unknown
): Promise<EditResult<typeof itemsTable.$inferSelect>> {
	const { set, reset } = parseRequest(body, ITEM_FIELDS);
	const [existing] = await db.select().from(itemsTable).where(eq(itemsTable.id, itemId));
	if (!existing) throw new ApiError(404, 'Unbekanntes Item');

	const current = existing as unknown as Record<string, unknown>;
	const patch: Record<string, unknown> = { ...set };
	// The sort key follows the title, so a corrected title also sorts where it belongs.
	if ('title' in set && !('sortTitle' in set)) {
		patch[DERIVED.title] = String(set.title ?? existing.title).toLowerCase();
	}

	const locked = new Set(existing.lockedFields ?? []);
	for (const column of Object.keys(set)) locked.add(column);
	if ('title' in set && !('sortTitle' in set)) locked.add(DERIVED.title);
	for (const column of reset) {
		locked.delete(column);
		if (column === 'title') locked.delete(DERIVED.title);
	}

	const batchId = randomUUID();
	const changed: string[] = [];
	const edits: (typeof metadataEdits.$inferInsert)[] = [];
	for (const [column, value] of Object.entries(patch)) {
		if (current[column] === value) continue;
		changed.push(column);
		edits.push({
			batchId,
			itemId,
			field: column,
			oldValue: asText(current[column]),
			newValue: asText(value),
			userId
		});
	}

	const [row] = await db
		.update(itemsTable)
		.set({ ...patch, lockedFields: [...locked].sort() })
		.where(eq(itemsTable.id, itemId))
		.returning();
	if (edits.length > 0) await db.insert(metadataEdits).values(edits);
	return { row, batchId, changed };
}

export async function editTrack(
	db: DrizzleDb,
	userId: number,
	trackId: number,
	body: unknown
): Promise<EditResult<typeof tracksTable.$inferSelect>> {
	const { set, reset } = parseRequest(body, TRACK_FIELDS);
	const [existing] = await db.select().from(tracksTable).where(eq(tracksTable.id, trackId));
	if (!existing) throw new ApiError(404, 'Unbekannter Titel');

	const current = existing as unknown as Record<string, unknown>;
	const locked = new Set(existing.lockedFields ?? []);
	for (const column of Object.keys(set)) locked.add(column);
	for (const column of reset) locked.delete(column);

	const batchId = randomUUID();
	const changed: string[] = [];
	const edits: (typeof metadataEdits.$inferInsert)[] = [];
	for (const [column, value] of Object.entries(set)) {
		if (current[column] === value) continue;
		changed.push(column);
		edits.push({
			batchId,
			trackId,
			field: column,
			oldValue: asText(current[column]),
			newValue: asText(value),
			userId
		});
	}

	const [row] = await db
		.update(tracksTable)
		.set({ ...set, lockedFields: [...locked].sort() })
		.where(eq(tracksTable.id, trackId))
		.returning();
	if (edits.length > 0) await db.insert(metadataEdits).values(edits);
	return { row, batchId, changed };
}
