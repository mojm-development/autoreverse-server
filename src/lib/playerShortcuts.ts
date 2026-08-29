/**
 * Keyboard shortcuts for the fullscreen player, kept apart from the page so the
 * mapping can be unit-tested without a DOM.
 */
export type PlayerAction = 'toggle' | 'back' | 'forward' | 'previous' | 'next' | 'close';

export interface ShortcutTarget {
	tagName?: string;
	isContentEditable?: boolean;
}

export interface ShortcutEvent {
	key: string;
	shiftKey?: boolean;
	ctrlKey?: boolean;
	metaKey?: boolean;
	altKey?: boolean;
	target?: ShortcutTarget | null;
}

/** Fields and anything else that answers to the keyboard keeps its own keys. */
const INTERACTIVE = new Set(['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON', 'A', 'OPTION']);

function isTyping(target: ShortcutTarget | null | undefined): boolean {
	if (!target) return false;
	if (target.isContentEditable) return true;
	return INTERACTIVE.has((target.tagName ?? '').toUpperCase());
}

export function shortcutFor(event: ShortcutEvent): PlayerAction | null {
	if (event.ctrlKey || event.metaKey || event.altKey) return null;
	if (isTyping(event.target)) return null;
	switch (event.key) {
		case ' ':
		case 'Spacebar':
		case 'k':
		case 'K':
			return 'toggle';
		case 'ArrowLeft':
			return event.shiftKey ? 'previous' : 'back';
		case 'ArrowRight':
			return event.shiftKey ? 'next' : 'forward';
		case 'p':
		case 'P':
			return 'previous';
		case 'n':
		case 'N':
			return 'next';
		case 'Escape':
			return 'close';
		default:
			return null;
	}
}
