import { page } from 'vitest/browser';
import { describe, it, expect, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Scrubber from './Scrubber.svelte';

function slider() {
	return document.querySelector('[role="slider"]') as HTMLElement;
}

function stubWidth(element: HTMLElement, left: number, width: number) {
	vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({
		left,
		width,
		right: left + width,
		top: 0,
		bottom: 10,
		height: 10,
		x: left,
		y: 0,
		toJSON: () => ({})
	} as DOMRect);
	element.setPointerCapture = () => {};
	element.releasePointerCapture = () => {};
}

function pointer(type: string, clientX: number) {
	return new PointerEvent(type, { clientX, bubbles: true, pointerId: 1 });
}

describe('Scrubber.svelte', () => {
	it('exposes its position to assistive technology', async () => {
		render(Scrubber, { value: 30, max: 120, onSeek: () => {}, format: (s) => `${s}s` });
		const bar = page.getByRole('slider');
		await expect.element(bar).toHaveAttribute('aria-valuenow', '30');
		await expect.element(bar).toHaveAttribute('aria-valuemax', '120');
		await expect.element(bar).toHaveAttribute('aria-valuetext', '30s');
	});

	it('seeks to the fraction of the bar that was clicked', async () => {
		const seeks: number[] = [];
		render(Scrubber, { value: 0, max: 200, onSeek: (s) => seeks.push(s) });
		const bar = slider();
		stubWidth(bar, 100, 400);

		bar.dispatchEvent(pointer('pointerdown', 200));
		bar.dispatchEvent(pointer('pointerup', 200));
		expect(seeks).toEqual([50]);
	});

	it('commits once on release, not on every move of the drag', async () => {
		const seeks: number[] = [];
		render(Scrubber, { value: 0, max: 100, onSeek: (s) => seeks.push(s) });
		const bar = slider();
		stubWidth(bar, 0, 100);

		bar.dispatchEvent(pointer('pointerdown', 10));
		bar.dispatchEvent(pointer('pointermove', 40));
		bar.dispatchEvent(pointer('pointermove', 70));
		expect(seeks).toEqual([]);
		bar.dispatchEvent(pointer('pointerup', 70));
		expect(seeks).toEqual([70]);
	});

	it('clamps a drag that leaves the bar on either side', async () => {
		const seeks: number[] = [];
		render(Scrubber, { value: 0, max: 100, onSeek: (s) => seeks.push(s) });
		const bar = slider();
		stubWidth(bar, 0, 100);

		bar.dispatchEvent(pointer('pointerdown', 50));
		bar.dispatchEvent(pointer('pointerup', -40));
		bar.dispatchEvent(pointer('pointerdown', 50));
		bar.dispatchEvent(pointer('pointerup', 400));
		expect(seeks).toEqual([0, 100]);
	});

	it('ignores a click while nothing is loaded', async () => {
		const seeks: number[] = [];
		render(Scrubber, { value: 0, max: 0, onSeek: (s) => seeks.push(s) });
		const bar = slider();
		stubWidth(bar, 0, 100);

		bar.dispatchEvent(pointer('pointerdown', 50));
		bar.dispatchEvent(pointer('pointerup', 50));
		expect(seeks).toEqual([]);
	});

	it('steps with the arrow keys and jumps with Home and End', async () => {
		const seeks: number[] = [];
		render(Scrubber, { value: 50, max: 100, onSeek: (s) => seeks.push(s), step: 5 });
		const bar = slider();

		bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
		bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
		bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }));
		bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
		expect(seeks).toEqual([55, 45, 0, 100]);
	});

	it('renders one tick per marker, positioned by its share of the duration', async () => {
		render(Scrubber, { value: 0, max: 200, onSeek: () => {}, ticks: [0, 50, 100] });
		const ticks = document.querySelectorAll('[role="slider"] span');
		expect(ticks).toHaveLength(3);
		expect((ticks[1] as HTMLElement).style.left).toBe('25%');
	});
});
