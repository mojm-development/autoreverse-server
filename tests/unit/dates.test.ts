import { describe, it, expect } from 'vitest';
import { relativeDay, humanDuration } from '../../src/lib/dates';

const now = new Date(2026, 7, 29, 9, 0); // 29 August 2026, 09:00 local

describe('relativeDay', () => {
	it('names the last few days instead of printing a date', () => {
		expect(relativeDay(new Date(2026, 7, 29, 1, 0).toISOString(), now)).toBe('Heute');
		expect(relativeDay(new Date(2026, 7, 28, 23, 30).toISOString(), now)).toBe('Gestern');
		expect(relativeDay(new Date(2026, 7, 25).toISOString(), now)).toBe('vor 4 Tagen');
	});

	it('counts calendar days, so an episode from an hour before midnight is yesterday', () => {
		const lateLastNight = new Date(2026, 7, 28, 23, 0).toISOString();
		// Less than 24 hours ago, but not today.
		expect(relativeDay(lateLastNight, now)).toBe('Gestern');
	});

	it('falls back to a written date beyond a week, and to nothing without a value', () => {
		expect(relativeDay(new Date(2026, 6, 1).toISOString(), now)).toContain('2026');
		expect(relativeDay(null, now)).toBe('');
		expect(relativeDay('not a date', now)).toBe('');
	});
});

describe('humanDuration', () => {
	it('reads as minutes below the hour and as hours plus minutes above it', () => {
		expect(humanDuration(48 * 60)).toBe('48 min');
		expect(humanDuration(3600 + 12 * 60)).toBe('1 Std 12 min');
		expect(humanDuration(7200)).toBe('2 Std');
	});

	it('says nothing for an unknown length and rounds a sliver up to <1 min', () => {
		expect(humanDuration(0)).toBe('');
		expect(humanDuration(-5)).toBe('');
		expect(humanDuration(20)).toBe('<1 min');
	});
});
