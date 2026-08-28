import { describe, it, expect } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Equalizer from './Equalizer.svelte';

function bars() {
	return document.querySelectorAll('.eq .bar');
}

describe('Equalizer.svelte', () => {
	it('runs its animation while something is playing', async () => {
		render(Equalizer, { playing: true });
		const first = bars()[0] as HTMLElement;
		expect(getComputedStyle(first).animationPlayState).toBe('running');
	});

	it('freezes when paused instead of disappearing', async () => {
		render(Equalizer, { playing: false });
		expect(bars()).toHaveLength(5);
		const first = bars()[0] as HTMLElement;
		expect(getComputedStyle(first).animationPlayState).toBe('paused');
	});

	it('tells assistive technology which state it is showing', async () => {
		render(Equalizer, { playing: true, label: 'Wiedergabe läuft' });
		const eq = document.querySelector('.eq')!;
		expect(eq.getAttribute('aria-label')).toBe('Wiedergabe läuft');
	});
});
