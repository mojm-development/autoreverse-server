import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import ChapterList from './ChapterList.svelte';

const CHAPTERS = [0, 1, 2].map((i) => ({
	title: `Ein Leben ist zu wenig - Die Autobiographie, Kapitel ${i + 1}`,
	start: i * 100,
	end: (i + 1) * 100
}));

function titles(): string[] {
	return Array.from(document.querySelectorAll('.title')).map((el) => el.textContent!.trim());
}

describe('ChapterList.svelte', () => {
	it('drops the book title that every chapter repeats', async () => {
		render(ChapterList, {
			chapters: CHAPTERS,
			currentPosition: 0,
			isPlayingThis: false,
			onSelect: () => {},
			itemTitle: 'Ein Leben ist zu wenig - Die Autobiographie (Gekürzt)'
		});
		expect(titles()).toEqual(['Kapitel 1', 'Kapitel 2', 'Kapitel 3']);
	});

	it('keeps every row on one line, whatever the title', async () => {
		render(ChapterList, {
			chapters: [{ title: 'Ein '.repeat(60), start: 0, end: 10 }],
			currentPosition: 0,
			isPlayingThis: false,
			onSelect: () => {}
		});
		const title = document.querySelector('.title')!;
		const row = document.querySelector('.row')!;
		// A wrapping title used to run over the chapter below it.
		expect(getComputedStyle(title).whiteSpace).toBe('nowrap');
		expect(title.getBoundingClientRect().height).toBeLessThan(row.getBoundingClientRect().height);
	});

	it('still announces the full title and plays from the chapter start', async () => {
		const onSelect = vi.fn();
		render(ChapterList, {
			chapters: CHAPTERS,
			currentPosition: 0,
			isPlayingThis: false,
			onSelect,
			itemTitle: 'Ein Leben ist zu wenig - Die Autobiographie (Gekürzt)'
		});
		const activator = document.querySelectorAll<HTMLButtonElement>('.activator')[1];
		// Screen readers get the real chapter title, not the shortened label.
		expect(activator.getAttribute('aria-label')).toContain('Die Autobiographie, Kapitel 2');
		activator.click();
		expect(onSelect).toHaveBeenCalledWith(100);
	});
});
