import { expect, test, type Page } from '@playwright/test';
import { TEST_PASSWORD, TEST_USER, itemId } from './_fixtures';

// The phone navigation (≤ 700px): tab bar with the five areas, the area's chips at the top of the
// content, a slim header with the account menu. Every library page is at most two taps from
// Start, and no page scrolls sideways.

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

const tabs = (page: Page) => page.getByRole('navigation', { name: 'Bereiche' });
const chips = (page: Page, area: string) =>
	page.getByRole('navigation', { name: area, exact: true });

// [tab, chip or null, the URL it lands on]
const DESTINATIONS: [string | RegExp, string | null, string][] = [
	['Start', null, '/library'],
	['Musik', 'Alben', '/library/albums'],
	['Musik', 'Interpreten', '/library/artists'],
	['Musik', 'Playlists', '/library/playlists'],
	['Musik', 'Favoriten', '/library/favorites'],
	[/^Podcasts/, 'Abos', '/library/podcasts'],
	[/^Podcasts/, 'Neue Folgen', '/library/podcasts?filter=new'],
	['Hörbücher', 'Bibliothek', '/library/books'],
	['Hörbücher', 'Serien', '/library/series'],
	['Hörbücher', 'Lesezeichen', '/library/bookmarks'],
	['Suche', null, '/library/search']
];
const AREA: Record<string, string> = { Musik: 'Musik', Hörbücher: 'Hörbücher' };

test('every library page is at most two taps from Start', async ({ page }) => {
	for (const [tab, chip, url] of DESTINATIONS) {
		await page.goto('/library');
		await tabs(page).getByRole('link', { name: tab }).tap();
		if (chip) {
			const area = typeof tab === 'string' ? AREA[tab] : 'Podcasts';
			await chips(page, area).getByRole('link', { name: chip, exact: true }).tap();
			await expect(chips(page, area).getByRole('link', { name: chip, exact: true })).toHaveAttribute(
				'aria-current',
				'true'
			);
		}
		await expect(page).toHaveURL(url);
		await expect(tabs(page).locator('a[aria-current="page"]')).toHaveCount(1);
		// The tab bar sits inside the viewport and every tab is a 44px target.
		for (const box of await tabs(page)
			.getByRole('link')
			.evaluateAll((links) => links.map((l) => l.getBoundingClientRect().toJSON()))) {
			expect(box.height).toBeGreaterThanOrEqual(44);
			expect(box.width).toBeGreaterThanOrEqual(44);
			expect(box.right).toBeLessThanOrEqual(390);
		}
	}
});

test('a podcast is two taps from Start: the Abos list shows on the phone too', async ({ page }) => {
	await page.goto('/library');
	await tabs(page).getByRole('link', { name: /^Podcasts/ }).tap();
	await page.getByRole('link', { name: /E2E-Seed-Sendung/ }).tap();
	await expect(page).toHaveURL(/\/library\/podcasts\/\d+$/);
	await expect(
		page.getByRole('heading', { level: 1, name: 'E2E-Seed-Sendung' })
	).toBeVisible();
	await expect(chips(page, 'Podcasts').getByRole('link', { name: 'Abos' })).toHaveAttribute(
		'aria-current',
		'true'
	);
});

test('a detail page keeps its area tab and chip', async ({ page, request }) => {
	await page.goto(`/library/albums/${await itemId(request, 'E2E-Fixture-Album-Detail')}`);
	await expect(tabs(page).getByRole('link', { name: 'Musik' })).toHaveAttribute('aria-current', 'page');
	await expect(chips(page, 'Musik').getByRole('link', { name: 'Alben' })).toHaveAttribute(
		'aria-current',
		'true'
	);
	await page.goto(`/library/books/${await itemId(request, 'E2E-Fixture-Book-WithProgress')}`);
	await expect(tabs(page).getByRole('link', { name: 'Hörbücher' })).toHaveAttribute(
		'aria-current',
		'page'
	);
	// The desktop sidebar is gone on a phone.
	await expect(page.locator('nav.sidebar')).toBeHidden();
});

test('no page scrolls sideways', async ({ page, request }) => {
	const album = await itemId(request, 'E2E-Fixture-Album-Detail');
	const book = await itemId(request, 'E2E-Fixture-Book-WithProgress');
	const res = await request.get('/playlists');
	const { playlists } = (await res.json()) as { playlists: { id: number; name: string }[] };
	const playlist = playlists.find((p) => p.name === 'E2E-Fixture-Playlist')!.id;
	await page.goto('/library/podcasts');
	const podcast = await page
		.locator('a[href^="/library/podcasts/"]')
		.filter({ hasText: 'E2E-Seed-Sendung' })
		.first()
		.getAttribute('href');
	expect(podcast).toMatch(/^\/library\/podcasts\/\d+$/);

	const pages = [
		...DESTINATIONS.map(([, , url]) => url),
		`/library/albums/${album}`,
		'/library/albums?view=list',
		'/library/artists/E2E%20Fixture%20Artist',
		`/library/playlists/${playlist}`,
		podcast!,
		'/library/podcasts/preview?feed=http%3A%2F%2F127.0.0.1%3A9%2Ffeed.xml',
		`/library/books/${book}`,
		'/library/books?view=list',
		'/library/search?q=E2E',
		'/settings',
		'/settings/libraries',
		'/settings/scan',
		'/settings/playback',
		'/settings/podcasts',
		'/settings/users',
		'/settings/security',
		'/settings/about'
	];
	for (const url of pages) {
		await page.goto(url);
		await expect(tabs(page)).toBeVisible();
		const { scrollWidth, innerWidth } = await page.evaluate(() => ({
			scrollWidth: document.documentElement.scrollWidth,
			innerWidth: window.innerWidth
		}));
		expect(scrollWidth, `${url} scrollt waagerecht`).toBeLessThanOrEqual(innerWidth);
	}
});

test('the Podcasts tab shows the unheard episodes as a badge', async ({ page }) => {
	await page.goto('/library');
	const podcasts = tabs(page).getByRole('link', { name: /^Podcasts/ });
	const badge = podcasts.locator('.badge');
	await expect(badge).toBeVisible();
	await expect(badge).toHaveText(/^[1-9]\d*$/);
	await expect(podcasts).toHaveAccessibleName(/neue Folgen$/);
});

test('settings pages leave every tab unmarked and the avatar shows where one is', async ({
	page
}) => {
	await page.goto('/settings/about');
	await expect(tabs(page).locator('a[aria-current]')).toHaveCount(0);
	await expect(page.getByLabel(`Konto: ${TEST_USER}`)).toHaveAttribute('data-here', '');
});

test.describe('account menu', () => {
	// Logging out revokes the session: log in fresh so the shared storage state stays valid (see
	// logout.e2e.ts).
	test.use({ storageState: { cookies: [], origins: [] } });

	test('opens from the avatar, closes outside, switches the theme and logs out', async ({
		page
	}) => {
		await page.goto('/login');
		await page.getByLabel('Name', { exact: false }).fill(TEST_USER);
		await page.getByLabel('Passwort').fill(TEST_PASSWORD);
		await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
		await expect(page).toHaveURL('/library');
		// The menu opens without script, but closing it needs static/js/pages.js to have run.
		await page.waitForLoadState('load');

		const avatar = page.getByLabel(`Konto: ${TEST_USER}`);
		const menu = page.locator('.m-menu');
		await expect(menu).toBeHidden();
		await avatar.tap();
		await expect(menu).toBeVisible();
		await expect(menu.getByText('Verwalter')).toBeVisible();
		await expect(menu.getByRole('link', { name: 'Einstellungen' })).toBeVisible();

		// A tap outside closes it again, and so does Esc.
		// The left edge of the greeting: outside the menu, which opens on the right.
		await page.getByRole('heading', { level: 1 }).tap({ position: { x: 4, y: 8 } });
		await expect(menu).toBeHidden();
		await avatar.tap();
		await expect(menu).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(menu).toBeHidden();

		// The theme switch keeps the menu open and names the next theme.
		await avatar.tap();
		const before = await page.evaluate(() => document.documentElement.dataset.theme ?? '');
		await menu.getByRole('button', { name: /Thema$/ }).tap();
		await expect(menu).toBeVisible();
		const after = await page.evaluate(() => document.documentElement.dataset.theme);
		expect(after).not.toBe(before);
		await expect(menu.getByRole('button', { name: /Thema$/ })).toHaveText(
			after === 'dark' ? 'Helles Thema' : 'Dunkles Thema'
		);

		await menu.getByRole('button', { name: 'Abmelden' }).tap();
		await expect(page).toHaveURL('/login');
		await page.goto('/library');
		await expect(page).toHaveURL('/login');
	});
});
