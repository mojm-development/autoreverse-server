import { describe, it, expect } from 'vitest';
import { shortcutFor, bindPlayerShortcuts } from '../../src/lib/playerShortcuts';

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

	it('binds and unbinds a keyboard target, swallowing the key it acted on', () => {
		let handler: ((event: KeyboardEvent) => void) | null = null;
		const target = {
			addEventListener: (_type: 'keydown', fn: (event: KeyboardEvent) => void) => {
				handler = fn;
			},
			removeEventListener: () => {
				handler = null;
			}
		};
		const seen: string[] = [];
		const stop = bindPlayerShortcuts((action) => seen.push(action), target);

		let prevented = 0;
		const press = (key: string) =>
			handler?.({
				key,
				target: null,
				preventDefault: () => (prevented += 1)
			} as unknown as KeyboardEvent);

		press(' ');
		press('x');
		expect(seen).toEqual(['toggle']);
		expect(prevented).toBe(1);

		stop();
		expect(handler).toBeNull();
	});
});
