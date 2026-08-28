import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Visualizer from './Visualizer.svelte';

function bars() {
	return Array.from(document.querySelectorAll<HTMLElement>('.viz .bar'));
}

function fakeAnalyser(level: number) {
	return {
		frequencyBinCount: 128,
		getByteFrequencyData: (target: Uint8Array) => target.fill(level)
	} as unknown as AnalyserNode;
}

/** A spectrum shaped like real music: loud bass, rolling off ~12 dB per decade. */
function slopedAnalyser() {
	const binCount = 1024;
	const sampleRate = 44100;
	const minDb = -62;
	const maxDb = -8;
	return {
		frequencyBinCount: binCount,
		context: { sampleRate },
		getByteFrequencyData: (target: Uint8Array) => {
			for (let bin = 0; bin < binCount; bin += 1) {
				const hz = Math.max(20, (bin * sampleRate) / 2 / binCount);
				const db = -18 - 12 * Math.log10(hz / 50);
				target[bin] = Math.max(
					0,
					Math.min(255, Math.round(((db - minDb) / (maxDb - minDb)) * 255))
				);
			}
		}
	} as unknown as AnalyserNode;
}

async function frames(count = 3) {
	for (let i = 0; i < count; i += 1) {
		await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
	}
}

describe('Visualizer.svelte', () => {
	it('falls back to the canned animation when no analyser is offered', async () => {
		render(Visualizer, { playing: true });
		expect(getComputedStyle(bars()[0]).animationPlayState).toBe('running');
	});

	it('freezes rather than disappearing when paused', async () => {
		render(Visualizer, { playing: false });
		expect(bars()).toHaveLength(5);
		expect(getComputedStyle(bars()[0]).animationPlayState).toBe('paused');
	});

	it('drives bar heights from the spectrum when an analyser is available', async () => {
		render(Visualizer, { playing: true, getAnalyser: () => fakeAnalyser(255) });
		await frames();
		const loud = bars().map((bar) => bar.style.height);
		expect(loud.every((height) => height !== '')).toBe(true);
		expect(parseFloat(loud[0])).toBeGreaterThan(90);
	});

	it('drops to the floor on silence instead of collapsing to nothing', async () => {
		render(Visualizer, { playing: true, getAnalyser: () => fakeAnalyser(0) });
		await frames();
		expect(parseFloat(bars()[0].style.height)).toBeGreaterThan(0);
		expect(parseFloat(bars()[0].style.height)).toBeLessThan(20);
	});

	it('keeps the canned animation when the analyser cannot be built', async () => {
		const getAnalyser = vi.fn(() => null);
		render(Visualizer, { playing: true, getAnalyser });
		await frames(2);
		expect(getAnalyser).toHaveBeenCalled();
		expect(getComputedStyle(bars()[0]).animationPlayState).toBe('running');
	});

	it('tells assistive technology which state it is showing', async () => {
		render(Visualizer, { playing: true, label: 'Wiedergabe läuft' });
		expect(document.querySelector('.viz')!.getAttribute('aria-label')).toBe('Wiedergabe läuft');
	});
	it('spreads a wide bar count across the spectrum without dead columns', async () => {
		render(Visualizer, {
			playing: true,
			bars: 40,
			getAnalyser: () => fakeAnalyser(200)
		});
		await frames();
		const heights = bars().map((bar) => parseFloat(bar.style.height));
		expect(heights).toHaveLength(40);
		expect(heights.every((height) => height > 20)).toBe(true);
	});

	it('splits the spectrum logarithmically instead of drowning in the bass', async () => {
		render(Visualizer, { playing: true, bars: 40, getAnalyser: slopedAnalyser });
		await frames();
		const heights = bars().map((bar) => parseFloat(bar.style.height));
		// A linear bin split used to pin most of the row to the ceiling.
		expect(heights.filter((height) => height > 95).length).toBeLessThan(4);
		// The bass end leads, the highs trail, and the first columns are not identical twins.
		expect(heights[0] - heights[39]).toBeGreaterThan(40);
		expect(new Set(heights.slice(0, 4)).size).toBe(4);
	});

	it('renders the number of bars it was asked for', async () => {
		render(Visualizer, { playing: false, bars: 12 });
		expect(bars()).toHaveLength(12);
	});
});
