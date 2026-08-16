import { existsSync } from 'node:fs';
import { defineConfig } from '@playwright/test';

// `npm run build && npm run preview` (the webServer command below) is a plain
// Node process tree, not `vite dev` — nothing loads .env into it automatically.
// src/lib/server/config.ts reads DATABASE_URL (and optionally AUTOREVERSE_DATA,
// which has a default) from process.env directly at module scope
// (src/lib/server/db/index.ts), and SvelteKit's build-time route analysis
// imports that module too, so the build step needs DATABASE_URL to be set.
// Playwright's webServer.env defaults to this process's process.env, so
// loading .env here (before defineConfig runs) is enough to pass it through.
if (existsSync('.env')) process.loadEnvFile('.env');

export default defineConfig({
	webServer: { command: 'npm run build && npm run preview', port: 4173 },
	testMatch: '**/*.e2e.{ts,js}',
	projects: [
		{ name: 'setup', testMatch: /auth\.setup\.ts/ },
		{ name: 'no-auth', testMatch: ['**/login.e2e.ts'] },
		{
			name: 'authenticated',
			testMatch: '**/*.e2e.{ts,js}',
			testIgnore: ['**/auth.setup.ts', '**/login.e2e.ts'],
			use: { storageState: 'playwright/.auth/user.json' },
			dependencies: ['setup']
		}
	]
});
