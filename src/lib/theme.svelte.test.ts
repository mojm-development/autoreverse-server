import { describe, it, expect, beforeEach } from 'vitest';
import { getTheme, setTheme } from './theme';

describe('theme', () => {
	beforeEach(() => localStorage.clear());
	it('defaults to no explicit override (follows prefers-color-scheme)', () => {
		expect(getTheme()).toBeNull();
	});
	it('setTheme persists and is readable back', () => {
		setTheme('light');
		expect(getTheme()).toBe('light');
		expect(document.documentElement.dataset.theme).toBe('light');
	});
});
