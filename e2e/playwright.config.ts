import { defineConfig } from '@playwright/test';

// The server is NOT started from here: it runs on the developer's Mac and the browser runs in a
// Tart VM on Jarvis (see run-on-jarvis.sh), so the base URL comes in from outside.
const baseURL = process.env.E2E_BASE_URL;
if (!baseURL) throw new Error('E2E_BASE_URL fehlt (z. B. http://10.5.0.77:3100)');

export default defineConfig({
	testDir: 'tests',
	testMatch: '**/*.e2e.ts',
	// One worker: several specs trigger real scans, and the scan status is one per server process
	// (POST /scan answers 409 while another scan runs) — parallel specs would race through it.
	workers: 1,
	reporter: [['list'], ['html', { open: 'never' }]],
	expect: { timeout: 10_000 },
	use: { baseURL, trace: 'retain-on-failure', screenshot: 'only-on-failure' },
	projects: [
		{ name: 'setup', testMatch: /auth\.setup\.ts/ },
		{ name: 'no-auth', testMatch: ['**/login.e2e.ts'] },
		{
			name: 'authenticated',
			testIgnore: ['**/auth.setup.ts', '**/login.e2e.ts'],
			use: { storageState: 'playwright/.auth/user.json' },
			dependencies: ['setup']
		}
	]
});
