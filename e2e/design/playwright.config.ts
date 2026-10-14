import { defineConfig } from '@playwright/test';

// Design comparison only (e2e/design/compare.sh): not part of the regular e2e suite. Each test
// opens its own contexts against both servers, so there is no baseURL here.
export default defineConfig({
	testDir: '.',
	testMatch: 'compare.spec.ts',
	workers: 2,
	timeout: 60_000,
	reporter: [['list']],
	expect: { timeout: 10_000 }
});
