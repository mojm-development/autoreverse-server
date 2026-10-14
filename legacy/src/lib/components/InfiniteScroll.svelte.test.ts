import { describe, it, expect, vi, afterEach } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { tick } from 'svelte';
import InfiniteScroll from './InfiniteScroll.svelte';

type Callback = (entries: { isIntersecting: boolean }[]) => void;

function stubObserver() {
	const instances: { callback: Callback; disconnected: boolean }[] = [];
	class FakeObserver {
		callback: Callback;
		constructor(callback: Callback) {
			this.callback = callback;
			instances.push({ callback, disconnected: false });
		}
		observe() {}
		disconnect() {
			const entry = instances.find((i) => i.callback === this.callback);
			if (entry) entry.disconnected = true;
		}
	}
	vi.stubGlobal('IntersectionObserver', FakeObserver);
	return {
		get live() {
			return instances.filter((i) => !i.disconnected);
		},
		fire() {
			for (const instance of instances.filter((i) => !i.disconnected)) {
				instance.callback([{ isIntersecting: true }]);
			}
		}
	};
}

describe('InfiniteScroll.svelte', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('asks for more when the sentinel comes into view', async () => {
		const observer = stubObserver();
		let calls = 0;
		render(InfiniteScroll, { onLoadMore: () => (calls += 1) });
		await tick();
		observer.fire();
		expect(calls).toBe(1);
	});

	it('watches nothing once the list is exhausted', async () => {
		const observer = stubObserver();
		let calls = 0;
		render(InfiniteScroll, { onLoadMore: () => (calls += 1), done: true });
		await tick();
		expect(observer.live).toHaveLength(0);
		observer.fire();
		expect(calls).toBe(0);
	});

	it('watches nothing while a page is already in flight', async () => {
		const observer = stubObserver();
		let calls = 0;
		render(InfiniteScroll, { onLoadMore: () => (calls += 1), loading: true });
		await tick();
		expect(observer.live).toHaveLength(0);
		expect(calls).toBe(0);
	});

	it('announces loading to assistive technology', async () => {
		stubObserver();
		render(InfiniteScroll, { onLoadMore: () => {}, loading: true, label: 'Weitere Alben' });
		await tick();
		const status = document.querySelector('[role="status"]')!;
		expect(status.getAttribute('aria-live')).toBe('polite');
		expect(status.textContent?.trim()).toBe('Weitere Alben…');
	});
});
