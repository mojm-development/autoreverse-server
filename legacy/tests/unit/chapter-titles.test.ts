import { describe, it, expect } from 'vitest';
import { chapterLabels } from '../../src/lib/chapterTitles';

describe('chapterLabels', () => {
	it('drops the book title an m4b repeats in every chapter', () => {
		const book = 'Ein Leben ist zu wenig - Die Autobiographie (Gekürzt)';
		const titles = [1, 2, 3].map(
			(n) => `Ein Leben ist zu wenig - Die Autobiographie, Kapitel ${n}`
		);
		expect(chapterLabels(titles, book)).toEqual(['Kapitel 1', 'Kapitel 2', 'Kapitel 3']);
	});

	it('drops a shared beginning even when the book title differs', () => {
		const titles = ['Teil A - Kapitel 1', 'Teil A - Kapitel 2', 'Teil A - Kapitel 3'];
		expect(chapterLabels(titles, 'Ganz anderer Titel')).toEqual([
			'Kapitel 1',
			'Kapitel 2',
			'Kapitel 3'
		]);
	});

	it('leaves real chapter names alone', () => {
		const titles = ['Die Ankunft', 'Der Sturm', 'Heimkehr'];
		expect(chapterLabels(titles, 'Der Schwarm')).toEqual(titles);
	});

	it('refuses to reduce chapters to bare numbers', () => {
		const titles = ['Kapitel 1', 'Kapitel 12', 'Kapitel 13'];
		// Dropping "Kapitel " would leave 1, 12, 13 — the row already shows a position,
		// and a number alone is not a title.
		expect(chapterLabels(titles)).toEqual(titles);
	});

	it('keeps a title that would otherwise vanish entirely', () => {
		const book = 'Das Parfum';
		const titles = ['Das Parfum', 'Das Parfum, Kapitel 2', 'Das Parfum, Kapitel 3'];
		// Cutting the shared prefix would leave the first chapter with nothing.
		expect(chapterLabels(titles, book)).toEqual(titles);
	});

	it('handles one chapter and none at all', () => {
		expect(chapterLabels([])).toEqual([]);
		expect(chapterLabels(['Nur eins'], 'Nur eins')).toEqual(['Nur eins']);
	});
});
