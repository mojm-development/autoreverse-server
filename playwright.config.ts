import { existsSync } from 'node:fs';
import { defineConfig } from '@playwright/test';

// `npm run build && npm run preview` (the webServer command below) is a plain
// Node process tree, not `vite dev` — nothing loads .env into it automatically.
// src/lib/server/config.ts reads CAPSTAN_BOOKS/CAPSTAN_MUSIC/DATABASE_URL from
// process.env directly at module scope (src/lib/server/db/index.ts), and
// SvelteKit's build-time route analysis imports that module too, so the build
// step itself fails with "CAPSTAN_BOOKS is not set" without this. Playwright's
// webServer.env defaults to this process's process.env, so loading .env here
// (before defineConfig runs) is enough to pass the values through.
if (existsSync('.env')) process.loadEnvFile('.env');

export default defineConfig({
	webServer: { command: 'npm run build && npm run preview', port: 4173 },
	testMatch: '**/*.e2e.{ts,js}'
});
