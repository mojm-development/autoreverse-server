import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import EpisodeCard from './EpisodeCard.svelte';

describe('EpisodeCard.svelte', () => {
	it('puts the title first and the rest in one quiet meta line', async () => {
		render(EpisodeCard, {
			title: 'Die große US-Open-Vorschau',
			date: 'Heute',
			duration: 3300,
			downloaded: true,
			onPlay: () => {}
		});
		expect(document.querySelector('.title')!.textContent).toContain('Die große US-Open-Vorschau');
		const meta = document.querySelector('.meta')!.textContent!;
		expect(meta).toContain('Heute');
		expect(meta).toContain('55 min');
		expect(meta).toContain('geladen');
	});

	it('shows what is left of a started episode, but not of a finished one', async () => {
		const { unmount } = render(EpisodeCard, {
			title: 'Angefangen',
			duration: 3600,
			position: 1800,
			onPlay: () => {}
		});
		expect(document.querySelector('.left')!.textContent).toContain('30 min');
		expect(document.querySelector('.fill')!.getAttribute('style')).toContain('50%');
		unmount();

		render(EpisodeCard, {
			title: 'Fertig',
			duration: 3600,
			position: 3600,
			finished: true,
			onPlay: () => {}
		});
		expect(document.querySelector('.progress-line')).toBeNull();
		expect(document.querySelector('.meta')!.textContent).toContain('gehört');
	});

	it('offers a download only while there is something to download', async () => {
		const { unmount } = render(EpisodeCard, {
			title: 'Nicht geladen',
			onPlay: () => {},
			onDownload: () => {}
		});
		expect(document.querySelector('.ghost-btn')).not.toBeNull();
		unmount();

		render(EpisodeCard, {
			title: 'Geladen',
			downloaded: true,
			onPlay: () => {},
			onDownload: () => {}
		});
		expect(document.querySelector('.ghost-btn')).toBeNull();
	});

	it('plays on click and says whether that means resuming', async () => {
		const onPlay = vi.fn();
		render(EpisodeCard, { title: 'Folge', duration: 100, position: 40, onPlay });
		const play = document.querySelector<HTMLButtonElement>('.play')!;
		expect(play.getAttribute('aria-label')).toBe('Folge fortsetzen');
		play.click();
		expect(onPlay).toHaveBeenCalled();
	});
});
