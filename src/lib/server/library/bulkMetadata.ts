import { randomUUID } from 'node:crypto';
import { and, eq, inArray, isNull, isNotNull, sql } from 'drizzle-orm';
import { items as itemsTable, metadataEdits } from '../db/schema';
import { ApiError } from '../api/errors';
import { ITEM_FIELDS } from '$lib/metadataFields';
import { likePattern } from './like';
import { asText, coerce, diffToEdits, nextLocks, LOCK_FIELD } from './metadata';
import type { DrizzleDb } from '../db';

/**
 * Editing many items at once.
 *
 * Two rules shape this: nothing is written before it has been shown (`dry_run`
 * returns the same diff the apply would perform), and everything written carries
 * one batch id so it can be taken back. A search-and-replace across two hundred
 * titles without either is a data loss waiting to happen.
 */

/** Above this, an accidental "select all" would rewrite the whole library. */
export const BULK_LIMIT = 2000;
/** How many rows a preview shows; the count is always complete. */
const PREVIEW_ROWS = 100;

export interface BulkFilter {
	kind?: string;
	q?: string;
	artist?: string;
	author?: string;
	series?: string;
	missing?: boolean;
}

export interface BulkRequest {
	ids?: number[];
	filter?: BulkFilter;
	set?: Record<string, unknown>;
	reset?: string[];
	replace?: { field: string; from: string; to: string; regex?: boolean };
	dry_run?: boolean;
}

export interface FieldChange {
	field: string;
	old: string | null;
	new: string | null;
}
export interface ItemChange {
	id: number;
	title: string;
	fields: FieldChange[];
}
export interface BulkResult {
	matched: number;
	/** Items whose values change. */
	changed: number;
	/** Items where only the lock changes — the value already was what you asked for. */
	locked_only: number;
	changes: ItemChange[];
	truncated: boolean;
	batch_id: string | null;
}

async function resolveTargets(db: DrizzleDb, request: BulkRequest) {
	if (request.ids && request.filter) {
		throw new ApiError(422, 'Entweder ids oder filter, nicht beides');
	}
	if (request.ids) {
		if (request.ids.length === 0) throw new ApiError(422, 'Keine Items ausgewählt');
		if (request.ids.some((id) => !Number.isInteger(id))) {
			throw new ApiError(422, 'Ungültige Item-ID');
		}
		if (request.ids.length > BULK_LIMIT) {
			throw new ApiError(422, `Höchstens ${BULK_LIMIT} Items auf einmal`);
		}
		return db.select().from(itemsTable).where(inArray(itemsTable.id, request.ids));
	}

	const filter = request.filter;
	if (!filter) throw new ApiError(422, 'Weder ids noch filter angegeben');
	const conditions = [isNull(itemsTable.parentId)];
	if (filter.kind) conditions.push(eq(itemsTable.kind, filter.kind));
	if (filter.artist) conditions.push(eq(itemsTable.artist, filter.artist));
	if (filter.author) conditions.push(eq(itemsTable.author, filter.author));
	if (filter.series) conditions.push(eq(itemsTable.series, filter.series));
	if (filter.q) {
		const pattern = likePattern(filter.q);
		conditions.push(
			sql`(${itemsTable.title} ILIKE ${pattern} ESCAPE '\\' OR ${itemsTable.author} ILIKE ${pattern} ESCAPE '\\' OR ${itemsTable.artist} ILIKE ${pattern} ESCAPE '\\')`
		);
	}
	if (filter.missing !== undefined) {
		conditions.push(
			filter.missing ? isNotNull(itemsTable.missingSince) : isNull(itemsTable.missingSince)
		);
	}
	const rows = await db
		.select()
		.from(itemsTable)
		.where(and(...conditions))
		.limit(BULK_LIMIT + 1);
	if (rows.length > BULK_LIMIT) {
		throw new ApiError(422, `Der Filter trifft mehr als ${BULK_LIMIT} Items`);
	}
	return rows;
}

function parseSet(raw: Record<string, unknown> | undefined) {
	const set: Record<string, string | number | null> = {};
	for (const [name, value] of Object.entries(raw ?? {})) {
		const spec = ITEM_FIELDS[name];
		if (!spec) throw new ApiError(422, `Unbekanntes Feld: ${name}`);
		set[spec.column] = coerce(name, spec, value);
	}
	return set;
}

function parseReset(raw: string[] | undefined) {
	return (raw ?? []).map((name) => {
		const spec = ITEM_FIELDS[name];
		if (!spec) throw new ApiError(422, `Unbekanntes Feld: ${name}`);
		return spec.column;
	});
}

function buildReplacer(replace: BulkRequest['replace']) {
	if (!replace) return null;
	const spec = ITEM_FIELDS[replace.field];
	if (!spec) throw new ApiError(422, `Unbekanntes Feld: ${replace.field}`);
	if (spec.type !== 'text') throw new ApiError(422, 'Ersetzen geht nur in Textfeldern');
	if (typeof replace.from !== 'string' || replace.from === '') {
		throw new ApiError(422, 'Suchtext fehlt');
	}
	const to = typeof replace.to === 'string' ? replace.to : '';
	if (replace.regex) {
		let pattern: RegExp;
		try {
			pattern = new RegExp(replace.from, 'g');
		} catch {
			throw new ApiError(422, 'Ungültiger regulärer Ausdruck');
		}
		return { column: spec.column, apply: (value: string) => value.replace(pattern, to) };
	}
	return { column: spec.column, apply: (value: string) => value.split(replace.from).join(to) };
}

export async function bulkEdit(db: DrizzleDb, userId: number, body: unknown): Promise<BulkResult> {
	if (!body || typeof body !== 'object') throw new ApiError(422, 'Ungültige Anfrage');
	const request = body as BulkRequest;
	const set = parseSet(request.set);
	const reset = parseReset(request.reset);
	const replacer = buildReplacer(request.replace);
	if (Object.keys(set).length === 0 && reset.length === 0 && !replacer) {
		throw new ApiError(422, 'Keine Änderungen angegeben');
	}

	const rows = await resolveTargets(db, request);
	const dryRun = request.dry_run !== false;
	const batchId = randomUUID();

	const changes: ItemChange[] = [];
	let lockedOnly = 0;
	const writes: { id: number; patch: Record<string, unknown>; locked: string[] }[] = [];
	const edits: (typeof metadataEdits.$inferInsert)[] = [];

	for (const row of rows) {
		const current = row as unknown as Record<string, unknown>;
		const patch: Record<string, unknown> = { ...set };
		if (replacer) {
			const before = current[replacer.column];
			if (typeof before === 'string') {
				const after = replacer.apply(before);
				if (after !== before) patch[replacer.column] = after.trim() === '' ? null : after;
			}
		}
		// A changed title takes its sort key with it, exactly as a single edit does.
		if ('title' in patch && !('sortTitle' in patch)) {
			patch.sortTitle = String(patch.title ?? row.title).toLowerCase();
		}

		const touched = Object.keys(patch);
		const locked = nextLocks(row.lockedFields ?? [], touched, reset);
		const diff = diffToEdits(current, patch, {
			batchId,
			itemId: row.id,
			userId,
			lockedBefore: row.lockedFields ?? [],
			lockedAfter: locked
		});
		const lockChanged = [...(row.lockedFields ?? [])].sort().join(',') !== locked.join(',');
		if (diff.changed.length === 0 && !lockChanged) continue;

		if (diff.changed.length > 0) {
			changes.push({
				id: row.id,
				title: row.title,
				fields: diff.edits
					.filter((edit) => edit.field !== LOCK_FIELD)
					.map((edit) => ({
						field: edit.field,
						old: edit.oldValue ?? null,
						new: edit.newValue ?? null
					}))
			});
		} else {
			// The value was already right; all this does is take the field off the
			// scanner. Worth writing, not worth showing as a diff.
			lockedOnly += 1;
		}
		writes.push({ id: row.id, patch, locked });
		edits.push(
			...(diff.edits.length > 0
				? diff.edits
				: [
						{
							batchId,
							itemId: row.id,
							field: LOCK_FIELD,
							oldValue: [...(row.lockedFields ?? [])].sort().join(','),
							newValue: locked.join(','),
							userId
						}
					])
		);
	}

	if (dryRun) {
		return {
			matched: rows.length,
			changed: changes.length,
			locked_only: lockedOnly,
			changes: changes.slice(0, PREVIEW_ROWS),
			truncated: changes.length > PREVIEW_ROWS,
			batch_id: null
		};
	}

	await db.transaction(async (tx) => {
		for (const write of writes) {
			await tx
				.update(itemsTable)
				.set({ ...write.patch, lockedFields: write.locked })
				.where(eq(itemsTable.id, write.id));
		}
		if (edits.length > 0) await tx.insert(metadataEdits).values(edits);
	});

	return {
		matched: rows.length,
		changed: changes.length,
		locked_only: lockedOnly,
		changes: changes.slice(0, PREVIEW_ROWS),
		truncated: changes.length > PREVIEW_ROWS,
		batch_id: writes.length > 0 ? batchId : null
	};
}

export interface UndoResult {
	restored: number;
	skipped: number;
}

/**
 * Puts a batch back. A field someone has changed since is left alone and counted as
 * skipped — an undo must not quietly discard work that came after it.
 */
export async function undoBatch(db: DrizzleDb, batchId: string): Promise<UndoResult> {
	const rows = await db.select().from(metadataEdits).where(eq(metadataEdits.batchId, batchId));
	if (rows.length === 0) throw new ApiError(404, 'Unbekannte Änderung');

	const byItem = new Map<number, typeof rows>();
	for (const row of rows) {
		if (row.itemId === null) continue;
		const list = byItem.get(row.itemId) ?? [];
		list.push(row);
		byItem.set(row.itemId, list);
	}

	let restored = 0;
	let skipped = 0;
	await db.transaction(async (tx) => {
		for (const [itemId, edits] of byItem) {
			const [current] = await tx.select().from(itemsTable).where(eq(itemsTable.id, itemId));
			if (!current) {
				skipped += edits.length;
				continue;
			}
			const row = current as unknown as Record<string, unknown>;
			const patch: Record<string, unknown> = {};
			for (const edit of edits) {
				if (edit.field === LOCK_FIELD) {
					patch.lockedFields = edit.oldValue ? edit.oldValue.split(',') : [];
					continue;
				}
				if (asText(row[edit.field]) !== edit.newValue) {
					skipped += 1;
					continue;
				}
				const spec = Object.values(ITEM_FIELDS).find((field) => field.column === edit.field);
				const value =
					edit.oldValue === null
						? null
						: spec && spec.type !== 'text'
							? Number(edit.oldValue)
							: edit.oldValue;
				patch[edit.field] = value;
				restored += 1;
			}
			if (Object.keys(patch).length > 0) {
				await tx.update(itemsTable).set(patch).where(eq(itemsTable.id, itemId));
			}
		}
		await tx.delete(metadataEdits).where(eq(metadataEdits.batchId, batchId));
	});

	return { restored, skipped };
}
