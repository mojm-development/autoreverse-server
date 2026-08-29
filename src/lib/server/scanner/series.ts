/**
 * Working out which series a book belongs to, and which volume it is.
 *
 * Nothing in an audiobook file is authoritative here: some rippers write the
 * series into ID3 movement/grouping frames (the Plex and Audiobookshelf
 * convention), most people encode it in the folder name, and the rest rely on
 * an Autor/Serie/Buch tree. All three are read, in that order of confidence.
 */

export interface SeriesGuess {
	series: string | null;
	index: number | null;
	/** The name with the series part removed — the plain book title. */
	rest: string | null;
}

const SEPARATOR = String.raw`[-–—:·]`;
/** "Band", "Teil", "Folge", "Vol." … the words that introduce a volume number. */
const VOLUME_WORD = String.raw`(?:band|teil|folge|nr\.?|no\.?|vol\.?|volume|book|buch)`;
const NUMBER = String.raw`(\d{1,4}(?:[.,]\d{1,2})?)`;

function toIndex(raw: string): number | null {
	const value = Number(raw.replace(',', '.'));
	return Number.isFinite(value) ? value : null;
}

function clean(value: string): string {
	return value
		.replace(new RegExp(`^\\s*${SEPARATOR}+\\s*`), '')
		.replace(new RegExp(`\\s*${SEPARATOR}+\\s*$`), '')
		.replace(/[\s,]+$/, '')
		.replace(/\s{2,}/g, ' ')
		.trim();
}

const PATTERNS: {
	re: RegExp;
	series: number;
	index: number;
	rest: number;
	/** Without a "Band"/"Folge" in front of it, a four-digit number is a year. */
	rejectYears?: boolean;
}[] = [
	// "Perry Rhodan, Band 3 - Titel" / "Perry Rhodan - Teil 3: Titel" / "Serie Vol. 3"
	{
		re: new RegExp(
			`^(.+?)[,\\s]*\\s*${SEPARATOR}?\\s*\\b${VOLUME_WORD}\\s*${NUMBER}\\b\\s*${SEPARATOR}?\\s*(.*)$`,
			'i'
		),
		series: 1,
		index: 2,
		rest: 3
	},
	// "Perry Rhodan #3 - Titel"
	{
		re: new RegExp(`^(.+?)\\s*#\\s*${NUMBER}\\s*${SEPARATOR}?\\s*(.*)$`),
		series: 1,
		index: 2,
		rest: 3
	},
	// "Perry Rhodan (3) - Titel"
	{
		re: new RegExp(`^(.+?)\\s*\\(\\s*${NUMBER}\\s*\\)\\s*${SEPARATOR}?\\s*(.*)$`),
		series: 1,
		index: 2,
		rest: 3
	},
	// "Der Herr der Ringe 01 - Die Gefährten": a number between name and title,
	// which only counts when a separator follows — otherwise "Album 2000" would match.
	{
		re: new RegExp(`^(.+?)\\s+${NUMBER}\\s*${SEPARATOR}\\s*(.+)$`),
		series: 1,
		index: 2,
		rest: 3,
		rejectYears: true
	}
];

/** A leading volume number: "03 - Der Fluch", "3. Der Fluch". */
const LEADING = new RegExp(`^${NUMBER}\\s*(?:${SEPARATOR}|\\.)\\s*(.+)$`);

/**
 * Reads a series and volume out of one name (a folder or an album tag).
 * Returns nulls when the name carries no series at all.
 */
export function parseSeriesName(name: string): SeriesGuess {
	const trimmed = name.trim();
	if (!trimmed) return { series: null, index: null, rest: null };

	for (const pattern of PATTERNS) {
		const match = trimmed.match(pattern.re);
		if (!match) continue;
		const series = clean(match[pattern.series]);
		const index = toIndex(match[pattern.index]);
		const rest = clean(match[pattern.rest] ?? '');
		// A series name of one or two characters is far more likely to be a stray number
		// than a real series.
		if (series.length < 3 || index === null) continue;
		// A bare 1984 between a name and a title is a year, not volume 1984.
		if (pattern.rejectYears && Number.isInteger(index) && index >= 1500 && index <= 2299) continue;
		return { series, index, rest: rest || null };
	}

	return { series: null, index: null, rest: null };
}

/** A volume number without a series name: the folder sits inside the series folder. */
export function parseLeadingIndex(name: string): { index: number | null; rest: string | null } {
	const match = name.trim().match(LEADING);
	if (!match) return { index: null, rest: null };
	const index = toIndex(match[1]);
	const rest = clean(match[2]);
	return index === null ? { index: null, rest: null } : { index, rest: rest || null };
}

export interface SeriesSource {
	/** Series written into the file's tags, if the ripper bothered. */
	tagSeries?: string | null;
	tagSeriesIndex?: number | null;
	/** The folder holding the audio files. */
	folderName: string;
	/** The folder above it — the series folder in an Autor/Serie/Buch tree. */
	parentName?: string | null;
	/** True when the tree is deep enough for the parent to be a series. */
	parentIsSeries?: boolean;
	/** The title the scanner would use otherwise (an album tag, usually). */
	title: string;
	/** Whether that title came from a tag rather than from the folder name. */
	titleFromTag?: boolean;
}

export interface SeriesResolution {
	series: string | null;
	seriesIndex: number | null;
	/** The title to store — stripped of the series prefix when it was in the folder name. */
	title: string;
}

/**
 * Tags first, then the folder name, then the shape of the tree. Every source can
 * fill in what an earlier one left open: an Autor/Serie/Buch tree that names its
 * book folder "03 - Titel" gives the series from the path and the volume from the
 * folder.
 */
export function resolveSeries(source: SeriesSource): SeriesResolution {
	const fromFolder = parseSeriesName(source.folderName);
	const leading = parseLeadingIndex(source.folderName);
	const fromTitle = source.titleFromTag ? parseSeriesName(source.title) : fromFolder;

	let series = source.tagSeries?.trim() || null;
	let index = source.tagSeriesIndex ?? null;
	let title = source.title;

	if (!series && source.parentIsSeries && source.parentName) {
		series = source.parentName;
		if (index === null) index = leading.index ?? fromFolder.index;
		// "03 - Der Fluch" inside a series folder: the number is not part of the title.
		if (!source.titleFromTag && leading.rest) title = leading.rest;
	}

	if (!series && fromFolder.series) {
		series = fromFolder.series;
		if (index === null) index = fromFolder.index;
		if (!source.titleFromTag && fromFolder.rest) title = fromFolder.rest;
	}

	if (!series && source.titleFromTag && fromTitle.series) {
		series = fromTitle.series;
		if (index === null) index = fromTitle.index;
		if (fromTitle.rest) title = fromTitle.rest;
	}

	if (series && index === null) {
		index = leading.index ?? fromFolder.index ?? (source.titleFromTag ? fromTitle.index : null);
	}

	return { series, seriesIndex: index, title };
}
