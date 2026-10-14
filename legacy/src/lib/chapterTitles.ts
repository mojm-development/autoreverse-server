/**
 * Chapter titles out of an m4b usually repeat the book: 146 chapters all called
 * "Ein Leben ist zu wenig - Die Autobiographie, Kapitel 3". In a list that belongs
 * to that book, the repetition is what pushes the useful part off the row — so the
 * shared beginning is dropped and only what tells the chapters apart is shown.
 */

const SEPARATORS = /^[\s\-–—:,.·|/]+/;
const TRAILING = /[\s\-–—:,.·|/]+$/;

function trimEdges(value: string): string {
	return value.replace(SEPARATORS, '').replace(TRAILING, '').trim();
}

/** Longest common prefix of two strings, cut back to a word boundary. */
function sharedPrefix(a: string, b: string): number {
	const max = Math.min(a.length, b.length);
	let i = 0;
	while (i < max && a[i].toLowerCase() === b[i].toLowerCase()) i += 1;
	// Never cut inside a word: "Kapitel 1" and "Kapitel 12" share "Kapitel 1".
	while (i > 0 && /[\p{L}\p{N}]/u.test(a[i - 1] ?? '') && /[\p{L}\p{N}]/u.test(a[i] ?? '')) i -= 1;
	return i;
}

/** Moves a cut back over one word, so the labels keep the word in front of a number. */
function backOneWord(text: string, cut: number): number {
	const match = text.slice(0, cut).match(/[\p{L}\p{N}]+[^\p{L}\p{N}]*$/u);
	return match ? cut - match[0].length : 0;
}

/** At least this much has to be shared before dropping it is worth anything. */
const MIN_ITEM_PREFIX = 10;
const MIN_COMMON_PREFIX = 6;
const MIN_CHAPTERS_FOR_COMMON = 3;

export function chapterLabels(titles: string[], itemTitle?: string | null): string[] {
	if (titles.length === 0) return [];
	let labels = [...titles];

	if (itemTitle) {
		const cut = titles.reduce(
			(least, title) => Math.min(least, sharedPrefix(title, itemTitle)),
			Number.MAX_SAFE_INTEGER
		);
		if (cut >= MIN_ITEM_PREFIX) {
			const shortened = labels.map((title) => trimEdges(title.slice(cut)));
			// A chapter that is nothing but the book's name keeps its own title.
			if (shortened.every((label) => label !== '')) labels = shortened;
		}
	}

	if (labels.length >= MIN_CHAPTERS_FOR_COMMON) {
		let cut = labels[0].length;
		for (const label of labels.slice(1)) cut = Math.min(cut, sharedPrefix(labels[0], label));
		// Bare numbers are no title: "Kapitel 3" says more than "3", and the row already
		// carries its position. Give the word before the number back and try again.
		const numeric = (list: string[]) => list.every((l) => /^\d+([.,]\d+)?$/.test(l));
		let shortened = labels.map((label) => trimEdges(label.slice(cut)));
		if (cut > 0 && numeric(shortened)) {
			cut = backOneWord(labels[0], cut);
			shortened = labels.map((label) => trimEdges(label.slice(cut)));
		}
		if (cut >= MIN_COMMON_PREFIX && !numeric(shortened) && shortened.every((l) => l !== '')) {
			labels = shortened;
		}
	}

	return labels;
}

/** The label for one chapter, in a list whose other titles are known. */
export function chapterLabel(
	title: string,
	allTitles: string[],
	itemTitle?: string | null
): string {
	const index = allTitles.indexOf(title);
	if (index < 0) return title;
	return chapterLabels(allTitles, itemTitle)[index] ?? title;
}
