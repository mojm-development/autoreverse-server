import { sql, type SQL } from 'drizzle-orm';
import type { AnyPgColumn } from 'drizzle-orm/pg-core';

type Sortable = AnyPgColumn | SQL;

const CHUNKS = '([0-9]+)|([^0-9]+)';

const NUMBER_WIDTH = 12;

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

const LABELLED =
	'(?:^|[^[:alnum:]])(?:folge|fall|teil|kapitel|episode|band|volume|vol|nummer|nr|part|chapter)[.]?[ ]*[#]?([0-9]{1,6})';

const LEADING = '^[ ]*([0-9]{1,6})[ ]*[-/._:][ ]*(?![0-9])';

export function episodeNumber(column: Sortable): SQL {
	return sql`coalesce(
		substring(lower(${column}) from ${LABELLED}::text)::int,
		substring(lower(${column}) from ${LEADING}::text)::int
	)`;
}
