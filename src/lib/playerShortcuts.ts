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

/**
 * Wires the shortcuts to a keyboard target (the window, normally). Returns the
 * teardown, so an `$effect` can hand it straight back. Only one binding should be
 * live at a time — the fullscreen player replaces the mini player's.
 */
export function bindPlayerShortcuts(
	run: (action: PlayerAction) => void,
	target: {
		addEventListener: (type: 'keydown', handler: (event: KeyboardEvent) => void) => void;
		removeEventListener: (type: 'keydown', handler: (event: KeyboardEvent) => void) => void;
	} = window
): () => void {
	function onKeydown(event: KeyboardEvent) {
		const action = shortcutFor({
			key: event.key,
			shiftKey: event.shiftKey,
			ctrlKey: event.ctrlKey,
			metaKey: event.metaKey,
			altKey: event.altKey,
			target: event.target as { tagName?: string; isContentEditable?: boolean } | null
		});
		if (!action) return;
		// The space bar would scroll the page, the arrows would too.
		event.preventDefault();
		run(action);
	}
	target.addEventListener('keydown', onKeydown);
	return () => target.removeEventListener('keydown', onKeydown);
}
