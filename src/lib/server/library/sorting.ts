import { sql, type SQL } from 'drizzle-orm';
import type { AnyPgColumn } from 'drizzle-orm/pg-core';

type Sortable = AnyPgColumn | SQL;

/** Splits a string into runs of digits and runs of non-digits. The alternation
 * yields exactly one non-null capture per match, so `m[1] IS NOT NULL` tells
 * the two apart. Neither branch can match the empty string, which is what
 * keeps `regexp_matches(..., 'g')` from spinning. */
const CHUNKS = '([0-9]+)|([^0-9]+)';

/** Digits are left-padded to this width so that lexicographic comparison of
 * the padded chunk equals numeric comparison. Six digits covers every track,
 * case and episode number in a media library; anything longer is padded to
 * its own length and compares as text again, which is a graceful degradation
 * rather than a wrong answer for the numbers that actually occur. */
const NUMBER_WIDTH = 12;

/**
 * A natural ("human") sort key: numbers inside the text compare numerically
 * instead of character by character.
 *
 * Plain `lower(sort_title)` puts TKKG's `100/…` between `10/…` and `11/…`,
 * because '0' < '1' at the third character. Zero-padding every digit run to a
 * fixed width and comparing the resulting chunk array fixes that:
 *
 *     '10/x'  -> {'000000000010','/x'}
 *     '100/x' -> {'000000000100','/x'}
 *     '11/x'  -> {'000000000011','/x'}
 *
 * Postgres compares `text[]` element by element, so this orders 10, 11, 100.
 *
 * Computed per row at query time rather than stored in `sort_title`: the
 * scanner's unchanged-folder fast path (scanner/store.ts) never rewrites
 * `sort_title`, so a stored key would stay stale for every folder whose files
 * have not been touched — i.e. for exactly the libraries that are already
 * scanned. The cost is that this cannot use the `item_kind_sort` index; at
 * a few thousand items that is not measurable, at six figures it would be.
 */
export function naturalKey(column: Sortable): SQL {
	return sql`(
		SELECT coalesce(
			array_agg(
				CASE WHEN chunk[1] IS NOT NULL THEN lpad(chunk[1], ${NUMBER_WIDTH}::int, '0') ELSE chunk[2] END
				ORDER BY ord
			),
			'{}'::text[]
		)
		FROM regexp_matches(lower(${column}), ${CHUNKS}::text, 'g') WITH ORDINALITY AS parts(chunk, ord)
	)`;
}

/** `Fall 04`, `Folge 12`, `Teil 3`, `Nr. 5`, `Vol. 2` — the number that names
 * an instalment. The label must be preceded by a non-alphanumeric (so
 * "Zufall 3" and "Anteil 2" don't count) and followed directly by an optional
 * dot/space/hash and then digits (so "Bookstore" and "Volumen" don't either). */
const LABELLED =
	'(?:^|[^[:alnum:]])(?:folge|fall|teil|kapitel|episode|band|volume|vol|nummer|nr|part|chapter)[.]?[ ]*[#]?([0-9]{1,6})';

/** A leading number used as the instalment marker instead of a label:
 * `04/Titel`, `04 - Titel`, `04. Titel`.
 *
 * Two guards, both load-bearing. A separator must follow the digits, so a
 * title that merely starts with a number ("1984") is not read as instalment
 * 1984. And the separator must not be followed by another digit, because
 * otherwise a grouped number reads as an instalment: "100,000,000 Bon Jovi
 * Fans Can't Be Wrong" became instalment 100, and German "1.000 Meilen"
 * would become instalment 1. Comma and semicolon are left out of the
 * separator class entirely for the same reason — no one numbers an
 * instalment "04, Titel", but thousands separators are everywhere. */
const LEADING = '^[ ]*([0-9]{1,6})[ ]*[-/._:][ ]*(?![0-9])';

/**
 * The instalment number carried in a title, or NULL when there is none.
 *
 * This exists because the number is the only thing the titles in a series
 * reliably agree on. A library where some folders are tagged
 * `Die neuen Fälle, Fall 04` and others `Sherlock Holmes - Die neuen Fälle,
 * Fall 04` splits into two alphabetical blocks no title sort can rejoin —
 * the strings differ at the first character. Ordering by the extracted
 * number instead ignores the prefix disagreement entirely.
 *
 * `items.series_index` would be the natural home for this, but the scanner
 * never writes it (`seriesIndex: null`, see scanner/books.ts), so deriving it
 * here keeps existing libraries working without a rescan.
 */
export function episodeNumber(column: Sortable): SQL {
	// The casts are not cosmetic: these patterns travel as bind parameters, and
	// without them Postgres has no context to infer $n's type from and rejects
	// the statement with "could not determine data type of parameter".
	return sql`coalesce(
		substring(lower(${column}) from ${LABELLED}::text)::int,
		substring(lower(${column}) from ${LEADING}::text)::int
	)`;
}
