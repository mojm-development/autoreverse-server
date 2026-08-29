import { describe, it, expect } from 'vitest';
import { shortcutFor } from '../../src/lib/playerShortcuts';

describe('shortcutFor', () => {
	it('maps the space bar to play/pause', () => {
		expect(shortcutFor({ key: ' ' })).toBe('toggle');
		expect(shortcutFor({ key: 'k' })).toBe('toggle');
	});

	it('seeks with the arrows and jumps tracks when shift is held', () => {
		expect(shortcutFor({ key: 'ArrowLeft' })).toBe('back');
		expect(shortcutFor({ key: 'ArrowRight' })).toBe('forward');
		expect(shortcutFor({ key: 'ArrowLeft', shiftKey: true })).toBe('previous');
		expect(shortcutFor({ key: 'ArrowRight', shiftKey: true })).toBe('next');
	});

	it('closes on escape', () => {
		expect(shortcutFor({ key: 'Escape' })).toBe('close');
	});

	it('keeps out of the way while a control has focus', () => {
		expect(shortcutFor({ key: ' ', target: { tagName: 'BUTTON' } })).toBeNull();
		expect(shortcutFor({ key: 'ArrowLeft', target: { tagName: 'SELECT' } })).toBeNull();
		expect(shortcutFor({ key: 'k', target: { tagName: 'INPUT' } })).toBeNull();
		expect(
			shortcutFor({ key: 'n', target: { tagName: 'DIV', isContentEditable: true } })
		).toBeNull();
	});

	it('leaves browser and system shortcuts alone', () => {
		expect(shortcutFor({ key: ' ', metaKey: true })).toBeNull();
		expect(shortcutFor({ key: 'ArrowRight', ctrlKey: true })).toBeNull();
		expect(shortcutFor({ key: 'x' })).toBeNull();
	});
});
