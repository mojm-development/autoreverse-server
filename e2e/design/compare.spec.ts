import { expect, test, type Browser, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';

// Design parity: every page of the web UI, screenshotted on the legacy SvelteKit server and on the
// Rust server with identical data (e2e/design/compare.sh seeds both), in two viewports and both
// themes. This spec only takes the pictures; the pixel diff runs on the Mac afterwards
// (e2e/design/diff.mjs), so the VM needs nothing beyond Playwright. e2e/design/styles.mjs is the
// browser-free counterpart: it compares the cascaded styles of both servers on the Mac.
//
//   SHOTS=books,album-detail  restricts the run to these shot names.

const SERVERS = { legacy: process.env.LEGACY_URL, rust: process.env.RUST_URL } as const;
if (!SERVERS.legacy || !SERVERS.rust) throw new Error('LEGACY_URL und RUST_URL setzen');
type Server = keyof typeof SERVERS;

const VIEWPORTS = {
	desktop: { width: 1440, height: 900 },
	mobile: { width: 390, height: 844 }
} as const;
const THEMES = ['dark', 'light'] as const;
const OUT = 'shots';
mkdirSync(OUT, { recursive: true });

async function itemId(page: Page, title: string): Promise<number> {
	const res = await page.request.get(`/items?q=${encodeURIComponent(title)}`);
	const { items } = (await res.json()) as { items: { id: number; title: string }[] };
	const item = items.find((i) => i.title === title);
	if (!item) throw new Error(`${title} fehlt`);
	return item.id;
}

async function playlistId(page: Page, name: string): Promise<number> {
	const res = await page.request.get('/playlists');
	const { playlists } = (await res.json()) as { playlists: { id: number; name: string }[] };
	return playlists.find((p) => p.name === name)!.id;
}

async function podcastHref(page: Page): Promise<string> {
	await page.goto('/library/podcasts');
	const href = await page
		.locator('a[href^="/library/podcasts/"]')
		.filter({ hasText: 'E2E-Fixture-Podcast' })
		.first()
		.getAttribute('href');
	return href!;
}

type Shot = {
	name: string;
	/** Path to open, or a function resolving it (ids differ between the two databases). */
	path: string | ((page: Page) => Promise<string>);
	/** Interaction after load (open a sheet, start playback, …). */
	then?: (page: Page) => Promise<void>;
	loggedOut?: boolean;
};

const FEED = encodeURIComponent('http://127.0.0.1:3101/feed.xml');

const SHOTS: Shot[] = [
	{ name: 'login', path: '/login', loggedOut: true },
	{ name: 'start', path: '/library' },
	{ name: 'books', path: '/library/books' },
	{ name: 'books-list', path: '/library/books?view=list' },
	{
		name: 'book-detail',
		path: async (p) => `/library/books/${await itemId(p, 'E2E-Fixture-Book-EndOfBook')}`
	},
	{ name: 'series', path: '/library/series' },
	{ name: 'albums', path: '/library/albums' },
	{ name: 'albums-list', path: '/library/albums?view=list' },
	{
		name: 'album-detail',
		path: async (p) => `/library/albums/${await itemId(p, 'E2E-Fixture-Album-Detail')}`
	},
	{ name: 'artists', path: '/library/artists' },
	{ name: 'artist-detail', path: '/library/artists/E2E%20Fixture%20Artist' },
	{ name: 'favorites', path: '/library/favorites' },
	{ name: 'bookmarks', path: '/library/bookmarks' },
	{ name: 'search', path: '/library/search?q=E2E-Fixture' },
	{ name: 'search-empty', path: '/library/search?q=nichtsdergleichen' },
	{ name: 'playlists', path: '/library/playlists' },
	{
		name: 'playlist-detail',
		path: async (p) => `/library/playlists/${await playlistId(p, 'E2E-Fixture-Playlist')}`
	},
	{ name: 'podcasts', path: '/library/podcasts' },
	{ name: 'podcast-detail', path: podcastHref },
	{ name: 'podcast-preview', path: `/library/podcasts/preview?feed=${FEED}` },
	{
		name: 'player',
		path: async (p) => `/library/${await itemId(p, 'E2E-Fixture-Book-EndOfBook')}/player`
	},
	{
		name: 'mini-player',
		path: async (p) => `/library/books/${await itemId(p, 'E2E-Fixture-NowPlaying-Book')}`,
		then: async (page) => {
			await page.getByRole('button', { name: 'Abspielen', exact: true }).click();
			await expect(page.getByTestId('mini-player')).toBeVisible();
			await page.waitForTimeout(800);
		}
	},
	{
		name: 'metadata-editor',
		path: async (p) => `/library/books/${await itemId(p, 'E2E-Fixture-Book-EndOfBook')}`,
		then: async (page) => {
			await page.getByRole('button', { name: 'Bearbeiten' }).click();
			await expect(page.getByRole('dialog', { name: 'Metadaten bearbeiten' })).toBeVisible();
			await page.waitForTimeout(500);
		}
	},
	{
		name: 'selection-bar',
		path: '/library/albums',
		then: async (page) => {
			await page.getByRole('button', { name: 'E2E-Bulk-Album-A auswählen' }).click();
			await page.getByRole('button', { name: 'E2E-Bulk-Album-B auswählen' }).click();
			await expect(page.getByRole('status').filter({ hasText: 'ausgewählt' })).toBeVisible();
		}
	},
	{
		name: 'bulk-editor',
		path: '/library/albums',
		then: async (page) => {
			await page.getByRole('button', { name: 'E2E-Bulk-Album-A auswählen' }).click();
			await page.getByRole('button', { name: 'E2E-Bulk-Album-B auswählen' }).click();
			await page
				.getByRole('status')
				.filter({ hasText: 'ausgewählt' })
				.getByRole('button', { name: 'Bearbeiten' })
				.click();
			await expect(page.getByRole('dialog', { name: 'Mehrere bearbeiten' })).toBeVisible();
			await page.waitForTimeout(500);
		}
	},
	{ name: 'settings', path: '/settings' },
	{ name: 'settings-libraries', path: '/settings/libraries' },
	{ name: 'settings-scan', path: '/settings/scan' },
	{ name: 'settings-playback', path: '/settings/playback' },
	{ name: 'settings-podcasts', path: '/settings/podcasts' },
	{ name: 'settings-users', path: '/settings/users' },
	{ name: 'settings-security', path: '/settings/security' },
	{ name: 'settings-about', path: '/settings/about' },
	{ name: 'not-found', path: '/library/books/999999' }
];

const only = process.env.SHOTS?.split(',').filter(Boolean);
const vpOnly = process.env.VIEWPORTS?.split(',').filter(Boolean);
const themeOnly = process.env.THEMES?.split(',').filter(Boolean);

async function login(browser: Browser, server: Server): Promise<string> {
	const file = `playwright/.auth/${server}.json`;
	const context = await browser.newContext({ baseURL: SERVERS[server] });
	const page = await context.newPage();
	await page.goto('/login');
	await page.getByLabel('Name', { exact: false }).fill('Oliver');
	await page.getByLabel('Passwort').fill('hunter2hunter2');
	await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
	await expect(page).toHaveURL(/\/library$/);
	await context.storageState({ path: file });
	await context.close();
	return file;
}

const auth: Partial<Record<Server, string>> = {};
test.beforeAll(async ({ browser }) => {
	mkdirSync('playwright/.auth', { recursive: true });
	auth.legacy = await login(browser, 'legacy');
	auth.rust = await login(browser, 'rust');
});

for (const shot of SHOTS) {
	if (only && !only.includes(shot.name)) continue;
	for (const [vp, viewport] of Object.entries(VIEWPORTS)) {
		if (vpOnly && !vpOnly.includes(vp)) continue;
		for (const theme of THEMES) {
			if (themeOnly && !themeOnly.includes(theme)) continue;
			test(`${shot.name} ${vp} ${theme}`, async ({ browser }) => {
				for (const server of ['legacy', 'rust'] as const) {
					const context = await browser.newContext({
						baseURL: SERVERS[server],
						viewport,
						colorScheme: theme,
						storageState: shot.loggedOut ? undefined : auth[server],
						reducedMotion: 'reduce'
					});
					await context.addInitScript((t) => localStorage.setItem('autoreverse-theme', t), theme);
					const page = await context.newPage();
					const path = typeof shot.path === 'string' ? shot.path : await shot.path(page);
					await page.goto(path);
					await page.waitForLoadState('networkidle');
					await page.evaluate(() => document.fonts.ready);
					if (shot.then) await shot.then(page);
					await page.mouse.move(0, 0);
					await page.screenshot({
						path: `${OUT}/${shot.name}__${vp}__${theme}__${server}.png`,
						fullPage: true,
						animations: 'disabled',
						caret: 'hide'
					});
					expect(page.url()).toBeTruthy();
					await context.close();
				}
			});
		}
	}
}
