import { describe, it, expect } from 'vitest';
import {
	parseSeriesName,
	parseLeadingIndex,
	resolveSeries
} from '../../src/lib/server/scanner/series';

describe('parseSeriesName', () => {
	it('reads the German shelf conventions', () => {
		expect(parseSeriesName('Perry Rhodan, Band 3 - Der Fluch')).toEqual({
			series: 'Perry Rhodan',
			index: 3,
			rest: 'Der Fluch'
		});
		expect(parseSeriesName('Die drei ??? - Folge 42: Der Karpatenhund')).toEqual({
			series: 'Die drei ???',
			index: 42,
			rest: 'Der Karpatenhund'
		});
		expect(parseSeriesName('Der Herr der Ringe 01 - Die Gefährten')).toEqual({
			series: 'Der Herr der Ringe',
			index: 1,
			rest: 'Die Gefährten'
		});
		expect(parseSeriesName('Discworld #5 - Sourcery')).toEqual({
			series: 'Discworld',
			index: 5,
			rest: 'Sourcery'
		});
		expect(parseSeriesName('Bobiverse (2) We Are Legion')).toEqual({
			series: 'Bobiverse',
			index: 2,
			rest: 'We Are Legion'
		});
	});

	it('keeps half-volumes, which is why the column is not an integer', () => {
		expect(parseSeriesName('Perry Rhodan, Band 3.5 - Zwischenspiel').index).toBe(3.5);
		expect(parseSeriesName('Perry Rhodan, Band 3,5 - Zwischenspiel').index).toBe(3.5);
	});

	it('leaves a plain title alone', () => {
		expect(parseSeriesName('Das Parfum')).toEqual({ series: null, index: null, rest: null });
		// A bare number after a name is a year or an edition, not a volume — without a
		// separator there is no title left for it to introduce.
		expect(parseSeriesName('Nevermind 1991')).toEqual({ series: null, index: null, rest: null });
		// Two characters cannot be a series name.
		expect(parseSeriesName('X 3 - Etwas')).toEqual({ series: null, index: null, rest: null });
		// A bare year reads as a year; with "Band" in front of it, it is a volume again.
		expect(parseSeriesName('Das Jahr 1984 - Roman')).toEqual({
			series: null,
			index: null,
			rest: null
		});
		expect(parseSeriesName('Perry Rhodan, Band 1984 - Titel')).toEqual({
			series: 'Perry Rhodan',
			index: 1984,
			rest: 'Titel'
		});
	});
});

describe('parseLeadingIndex', () => {
	it('picks the volume off a folder that sits inside its series', () => {
		expect(parseLeadingIndex('03 - Der Fluch')).toEqual({ index: 3, rest: 'Der Fluch' });
		expect(parseLeadingIndex('7. Der Fluch')).toEqual({ index: 7, rest: 'Der Fluch' });
		expect(parseLeadingIndex('Der Fluch')).toEqual({ index: null, rest: null });
	});
});

describe('resolveSeries', () => {
	it('believes the tags before anything else', () => {
		expect(
			resolveSeries({
				tagSeries: 'Perry Rhodan',
				tagSeriesIndex: 12,
				folderName: 'Irgendein Ordner',
				title: 'Der Fluch',
				titleFromTag: true
			})
		).toEqual({ series: 'Perry Rhodan', seriesIndex: 12, title: 'Der Fluch' });
	});

	it('takes the series from the tree and the volume from the folder', () => {
		expect(
			resolveSeries({
				folderName: '03 - Der Fluch',
				parentName: 'Die drei ???',
				parentIsSeries: true,
				title: '03 - Der Fluch'
			})
		).toEqual({ series: 'Die drei ???', seriesIndex: 3, title: 'Der Fluch' });
	});

	it('reads both out of the folder name when the tree is flat', () => {
		expect(
			resolveSeries({
				folderName: 'Der Herr der Ringe 01 - Die Gefährten',
				title: 'Der Herr der Ringe 01 - Die Gefährten'
			})
		).toEqual({ series: 'Der Herr der Ringe', seriesIndex: 1, title: 'Die Gefährten' });
	});

	it('does not rewrite a title that came from a tag', () => {
		const resolved = resolveSeries({
			folderName: 'Der Herr der Ringe 01 - Die Gefährten',
			title: 'Die Gefährten',
			titleFromTag: true
		});
		expect(resolved.series).toBe('Der Herr der Ringe');
		expect(resolved.seriesIndex).toBe(1);
		expect(resolved.title).toBe('Die Gefährten');
	});

	it('leaves a standalone book without a series', () => {
		expect(resolveSeries({ folderName: 'Das Parfum', title: 'Das Parfum' })).toEqual({
			series: null,
			seriesIndex: null,
			title: 'Das Parfum'
		});
	});
});
