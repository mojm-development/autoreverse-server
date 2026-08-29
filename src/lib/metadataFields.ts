/**
 * The fields the metadata editor may touch, shared by the API (which validates
 * against this) and the editor UI (which renders it). One definition, so a field
 * cannot exist in the form but be refused by the server.
 */
export type FieldType = 'text' | 'int' | 'float';

export interface MetadataField {
	/** snake_case, as it travels over the wire. */
	wire: string;
	/** Column name in the schema — this is what `locked_fields` stores. */
	column: string;
	type: FieldType;
	label: string;
	kinds: ('book' | 'album')[];
	max?: number;
	hint?: string;
}

export const ITEM_FIELD_LIST: MetadataField[] = [
	{
		wire: 'title',
		column: 'title',
		type: 'text',
		label: 'Titel',
		kinds: ['book', 'album'],
		max: 500
	},
	{
		wire: 'sort_title',
		column: 'sortTitle',
		type: 'text',
		label: 'Sortiertitel',
		kinds: ['book', 'album'],
		max: 500,
		hint: 'Leer lassen — folgt dem Titel, außer du willst bewusst anders einsortieren.'
	},
	{ wire: 'author', column: 'author', type: 'text', label: 'Autor', kinds: ['book'], max: 300 },
	{
		wire: 'narrator',
		column: 'narrator',
		type: 'text',
		label: 'Sprecher',
		kinds: ['book'],
		max: 300
	},
	{
		wire: 'artist',
		column: 'artist',
		type: 'text',
		label: 'Interpret',
		kinds: ['album'],
		max: 300
	},
	{
		wire: 'album_artist',
		column: 'albumArtist',
		type: 'text',
		label: 'Album-Interpret',
		kinds: ['album'],
		max: 300
	},
	{ wire: 'series', column: 'series', type: 'text', label: 'Serie', kinds: ['book'], max: 300 },
	{ wire: 'series_index', column: 'seriesIndex', type: 'float', label: 'Band', kinds: ['book'] },
	{ wire: 'year', column: 'year', type: 'int', label: 'Jahr', kinds: ['book', 'album'] }
];

export const TRACK_FIELD_LIST: MetadataField[] = [
	{
		wire: 'title',
		column: 'title',
		type: 'text',
		label: 'Titel',
		kinds: ['book', 'album'],
		max: 500
	},
	{ wire: 'disc', column: 'disc', type: 'int', label: 'CD', kinds: ['album'] }
];

function byWire(list: MetadataField[]): Record<string, MetadataField> {
	return Object.fromEntries(list.map((field) => [field.wire, field]));
}

export const ITEM_FIELDS = byWire(ITEM_FIELD_LIST);
export const TRACK_FIELDS = byWire(TRACK_FIELD_LIST);

export function fieldsFor(kind: string): MetadataField[] {
	return ITEM_FIELD_LIST.filter((field) => field.kinds.includes(kind as 'book' | 'album'));
}
