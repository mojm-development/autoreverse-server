// Keyboard shortcuts of the players (legacy src/lib/playerShortcuts.ts), kept apart so the
// mapping can be tested without a DOM.

/** Fields and anything else that answers to the keyboard keeps its own keys. */
const INTERACTIVE = new Set(['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON', 'A', 'OPTION']);

function isTyping(target) {
	if (!target) return false;
	if (target.isContentEditable) return true;
	return INTERACTIVE.has((target.tagName ?? '').toUpperCase());
}

/** 'toggle' | 'back' | 'forward' | 'previous' | 'next' | 'close' | null */
export function shortcutFor(event) {
	if (event.ctrlKey || event.metaKey || event.altKey) return null;
	if (isTyping(event.target)) return null;
	// The scrubber is a slider: its arrow keys seek by themselves.
	if (event.target?.getAttribute?.('role') === 'slider' && event.key.startsWith('Arrow')) return null;
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

/** Wires the shortcuts to a keyboard target; returns the teardown. */
export function bindPlayerShortcuts(run, target = window) {
	function onKeydown(event) {
		const action = shortcutFor(event);
		if (!action) return;
		if (!run(action)) return;
		// The space bar would scroll the page, the arrows would too.
		event.preventDefault();
	}
	target.addEventListener('keydown', onKeydown);
	return () => target.removeEventListener('keydown', onKeydown);
}
