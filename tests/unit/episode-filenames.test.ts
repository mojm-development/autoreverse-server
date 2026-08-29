import { describe, it, expect } from 'vitest';
import { safeFileName } from '../../src/lib/server/podcasts/download';

describe('safeFileName', () => {
	it('keeps a plain title unchanged', () => {
		expect(safeFileName('Folge 12 – Über Bäume', 'fallback')).toBe('Folge 12 – Über Bäume');
	});

	it('replaces separators and characters that break on Windows shares', () => {
		expect(safeFileName('AC/DC: Was nun? <live>', 'fallback')).toBe('AC DC Was nun live');
		expect(safeFileName('a\\b|c*d"e', 'fallback')).toBe('a b c d e');
	});

	it('drops control codes and collapses the whitespace they leave behind', () => {
		expect(safeFileName('Folge\u00071\u0007  Auftakt', 'fallback')).toBe('Folge 1 Auftakt');
	});

	it('trims trailing dots and spaces, which Windows would strip on its own', () => {
		expect(safeFileName('Fortsetzung folgt...  ', 'fallback')).toBe('Fortsetzung folgt');
	});

	it('caps the length so the whole path stays within filesystem limits', () => {
		expect(safeFileName('x'.repeat(300), 'fallback')).toHaveLength(120);
	});

	it('falls back when nothing usable is left', () => {
		expect(safeFileName('', '42')).toBe('42');
		expect(safeFileName('///', '42')).toBe('42');
		expect(safeFileName('   ', '42')).toBe('42');
	});
});
